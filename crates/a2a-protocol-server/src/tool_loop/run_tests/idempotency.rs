// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Idempotency keys and repeated calls (ADR 0026).

use a2a_protocol_types::message::Message;
use a2a_protocol_types::task::TaskId;

use super::*;
use crate::request_context::RequestContext;
use crate::tool_loop::ToolLoopExecutor;
use crate::tool_loop::idempotency::{call_key, task_scope};

const SECRET: [u8; 32] = [3; 32];

/// `restart`, a tool with a side effect, answering `restarted <service>`.
fn restart_tool() -> (TestTool, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
    let (tool, calls) = TestTool::new("restart", |args| {
        Ok(format!(
            "restarted {}",
            args.get("service").and_then(Value::as_str).unwrap_or("?")
        ))
    });
    let keys = Arc::clone(&tool.keys);
    (tool, calls, keys)
}

#[tokio::test]
async fn an_identical_call_to_a_tool_with_side_effects_runs_once() {
    let (restart, used, keys) = restart_tool();
    let (model, seen) = Scripted::new(vec![
        // Twice in one turn, then again on the next with its fields written
        // the other way round: the same call each time.
        calls(vec![
            call("a", "restart", r#"{"service":"billing","force":true}"#),
            call("b", "restart", r#"{"service":"billing","force":true}"#),
        ]),
        calls(vec![call(
            "c",
            "restart",
            r#"{"force":true,"service":"billing"}"#,
        )]),
        text("billing was restarted"),
    ]);
    let tool_loop = ToolLoop::new(model, tools(vec![restart]));
    let answer = run(&tool_loop).await.unwrap();

    assert_eq!(used.load(Ordering::SeqCst), 1);
    assert_eq!(keys.lock().unwrap().len(), 1);
    let replayed = "not run again: an identical call already ran in this task and \
                    returned: restarted billing";
    assert_eq!(
        results(&seen.lock().unwrap(), 2),
        vec![
            ("a".to_owned(), "restarted billing".to_owned()),
            ("b".to_owned(), replayed.to_owned()),
            ("c".to_owned(), replayed.to_owned()),
        ]
    );
    assert_eq!(answer.trace.len(), 3);
    assert!(answer.trace[1].ends_with(replayed), "{:?}", answer.trace);
}

#[tokio::test]
async fn different_arguments_are_different_calls_with_different_keys() {
    let (restart, used, keys) = restart_tool();
    let (model, _) = Scripted::new(vec![
        calls(vec![
            call("a", "restart", r#"{"service":"billing"}"#),
            call("b", "restart", r#"{"service":"checkout"}"#),
        ]),
        text("done"),
    ]);
    run(&ToolLoop::new(model, tools(vec![restart])))
        .await
        .unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 2);
    let keys = keys.lock().unwrap().clone();
    assert_ne!(keys[0], keys[1]);
    assert!(keys.iter().all(|k| k.len() == 64));
}

#[tokio::test]
async fn a_read_only_tool_runs_every_time_with_the_same_key() {
    let (status, used) = status_tool();
    let keys = Arc::clone(&status.keys);
    let checkout = r#"{"service":"checkout"}"#;
    let (model, _) = Scripted::new(vec![
        calls(vec![call("a", "service_status", checkout)]),
        calls(vec![call("b", "service_status", checkout)]),
        text("healthy"),
    ]);
    run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 2);
    let keys = keys.lock().unwrap().clone();
    assert_eq!(keys[0], keys[1]);
}

#[tokio::test]
async fn a_call_that_failed_is_run_again_with_the_same_key() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen_attempts = Arc::clone(&attempts);
    let (flaky, used) = TestTool::new("notify", move |_| {
        if seen_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(ToolError::Failed("the mail relay timed out".to_owned()))
        } else {
            Ok("sent".to_owned())
        }
    });
    let keys = Arc::clone(&flaky.keys);
    let args = r#"{"to":"ops"}"#;
    let (model, _) = Scripted::new(vec![
        calls(vec![call("a", "notify", args)]),
        calls(vec![call("b", "notify", args)]),
        calls(vec![call("c", "notify", args)]),
        text("sent"),
    ]);
    let answer = run(&ToolLoop::new(model, tools(vec![flaky])))
        .await
        .unwrap();
    // The failure ran, the retry ran with the same key so a backend that
    // did the work before failing can recognise it, and the third call was
    // answered from the success.
    assert_eq!(used.load(Ordering::SeqCst), 2);
    let keys = keys.lock().unwrap().clone();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], keys[1]);
    assert!(
        answer.trace[2].contains("not run again"),
        "{:?}",
        answer.trace
    );
}

#[tokio::test]
async fn keys_follow_the_scope_and_the_secret() {
    async fn key_of(secret: Option<[u8; 32]>, scope: Option<&str>) -> String {
        let (restart, _, keys) = restart_tool();
        let (model, _) = Scripted::new(vec![
            calls(vec![call("a", "restart", r#"{"service":"billing"}"#)]),
            text("done"),
        ]);
        let mut tool_loop = ToolLoop::new(model, tools(vec![restart]));
        if let Some(secret) = secret {
            tool_loop = tool_loop.with_idempotency_secret(secret);
        }
        let cancel = CancellationToken::new();
        match scope {
            Some(scope) => tool_loop.run_scoped(scope, "go", &cancel, None).await,
            None => tool_loop.run("go", &cancel).await,
        }
        .unwrap();
        keys.lock().unwrap()[0].clone()
    }

    // Two loops sharing a secret, as replicas would, agree in one scope.
    let shared = key_of(Some(SECRET), Some("task-1")).await;
    assert_eq!(shared, key_of(Some(SECRET), Some("task-1")).await);
    assert_eq!(
        shared,
        call_key(&SECRET, "task-1", "restart", &json!({"service": "billing"}))
    );
    assert_ne!(shared, key_of(Some(SECRET), Some("task-2")).await);
    assert_ne!(shared, key_of(Some([4; 32]), Some("task-1")).await);
    // By default each loop has its own secret, and each run its own scope.
    assert_ne!(
        key_of(None, Some("task-1")).await,
        key_of(None, Some("task-1")).await
    );
    assert_ne!(
        key_of(Some(SECRET), None).await,
        key_of(Some(SECRET), None).await
    );
}

#[tokio::test]
async fn the_executor_scopes_keys_to_the_task_and_the_message() {
    let (restart, _, keys) = restart_tool();
    let (model, _) = Scripted::new(vec![
        calls(vec![call("a", "restart", r#"{"service":"billing"}"#)]),
        text("done"),
    ]);
    let executor = ToolLoopExecutor::new(
        ToolLoop::new(model, tools(vec![restart])).with_idempotency_secret(SECRET),
    );
    let ctx = RequestContext::new(
        Message::user_text("m-7", "restart billing"),
        TaskId::new("t-9"),
        "c-1".to_owned(),
    );
    let (writer, _reader) = crate::streaming::event_queue::new_in_memory_queue();
    crate::AgentExecutor::execute(&executor, &ctx, &writer)
        .await
        .unwrap();
    assert_eq!(
        keys.lock().unwrap()[0],
        call_key(
            &SECRET,
            &task_scope("t-9", "m-7"),
            "restart",
            &json!({"service": "billing"})
        )
    );
}
