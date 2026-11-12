// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A tool call is on the audit record before it runs, and does not run if it
//! cannot be put there (adversarial review A3, 2026-11-12).

use super::strict::{Gate, identity, mutating, run_as};
use super::*;
use crate::audit::tests::outcome_durability::Flaky;
use crate::audit::{AuditLog, AuditStore};
use crate::tool_loop::AuditedPolicy;
use std::sync::Mutex as StdMutex;

fn required_log(store: &Arc<Flaky>) -> Arc<AuditLog> {
    Arc::new(AuditLog::new(Arc::clone(store) as Arc<dyn AuditStore>).require_record(true))
}

const SECRET: &str = "ssn-078-05-1120";

fn one_transfer() -> Vec<Result<ModelTurn, ModelError>> {
    vec![
        calls(vec![call(
            "c1",
            "transfer",
            &format!(r#"{{"service":"{SECRET}"}}"#),
        )]),
        text("done"),
    ]
}

/// The record is in the store when the tool starts, names the tool, the
/// caller, the task and a digest of the arguments, and holds no argument.
#[tokio::test]
async fn an_allowed_call_is_on_the_record_before_it_runs() {
    let store = Flaky::new();
    let seen_by_the_tool = Arc::new(StdMutex::new(None));
    let (transfer, used) = TestTool::new("transfer", {
        let store = Arc::clone(&store);
        let seen = Arc::clone(&seen_by_the_tool);
        move |_| {
            *seen.lock().unwrap() = Some(store.stored.load(Ordering::SeqCst));
            Ok("done".to_owned())
        }
    });
    let log = required_log(&store);
    let (model, _) = Scripted::new(one_transfer());
    let tool_loop = ToolLoop::new(model, tools(vec![transfer]))
        .with_policy(AuditedPolicy::new(Arc::clone(&log)));
    run_as(&tool_loop, &identity("alice")).await.unwrap();

    assert_eq!(used.load(Ordering::SeqCst), 1);
    assert_eq!(
        *seen_by_the_tool.lock().unwrap(),
        Some(1),
        "the record was not in the store when the tool began"
    );
    let records = log.export("").await.unwrap();
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.kind, "tool.action");
    assert_eq!(r.task_id.as_deref(), Some("task-1"));
    assert_eq!(r.actor.as_ref().map(|a| a.subject.as_str()), Some("alice"));
    assert_eq!(r.detail["tool"], "transfer");
    assert_eq!(r.detail["toolCallId"], "c1");
    assert_eq!(r.detail["decision"], "allow");
    assert_eq!(r.detail["recoveredFromText"], false);
    let digest = r
        .digests
        .get("arguments")
        .expect("a digest of the arguments");
    assert!(digest.starts_with("sha256:"), "{digest}");
    let everything = serde_json::to_string(r).unwrap();
    assert!(!everything.contains(SECRET), "an argument was recorded");
    assert!(log.verify("").await.unwrap().is_intact());
}

/// A store that refuses the record stops the call: the tool does not run, the
/// model is told in words it can act on, and the run goes on.
#[tokio::test]
async fn a_call_that_cannot_be_recorded_does_not_run() {
    let store = Flaky::new();
    store.set_down(true);
    let (transfer, used) = mutating();
    let log = required_log(&store);
    let (model, seen) = Scripted::new(one_transfer());
    let tool_loop = ToolLoop::new(model, tools(vec![transfer]))
        .with_policy(AuditedPolicy::new(Arc::clone(&log)));
    let answer = run_as(&tool_loop, &identity("alice")).await.unwrap();

    assert_eq!(used.load(Ordering::SeqCst), 0, "an unrecorded action ran");
    assert_eq!(answer.text, "done");
    let told = results(&seen.lock().unwrap().clone(), 1);
    assert!(told[0].1.contains("could not be recorded"), "{told:?}");
    assert_eq!(log.failures(), 1);
    assert_eq!(
        log.pending_outcomes(),
        0,
        "an action that did not run is not owed a record later"
    );
}

/// A call that is not recorded is not retried into the record later either:
/// healing the store afterwards writes nothing for the call that did not run.
#[tokio::test(start_paused = true)]
async fn a_refused_action_is_not_written_after_the_store_heals() {
    let store = Flaky::new();
    store.set_down(true);
    let (transfer, _) = mutating();
    let log = required_log(&store);
    let (model, _) = Scripted::new(one_transfer());
    let tool_loop = ToolLoop::new(model, tools(vec![transfer]))
        .with_policy(AuditedPolicy::new(Arc::clone(&log)));
    run_as(&tool_loop, &identity("alice")).await.unwrap();
    store.set_down(false);
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    assert_eq!(log.export("").await.unwrap().len(), 0);
}

/// An inner policy decides first; its denial is recorded as a denial and the
/// call does not run. Its allowance is recorded and the call runs.
#[tokio::test]
async fn the_wrapped_policy_decides_and_both_verdicts_are_recorded() {
    let store = Flaky::new();
    let log = required_log(&store);
    let asked = Arc::new(StdMutex::new(Vec::new()));
    for (who, runs, decision) in [("mallory", 0, "deny"), ("alice", 1, "allow")] {
        let (transfer, used) = mutating();
        let (model, _) = Scripted::new(vec![
            calls(vec![call("c1", "transfer", "{}")]),
            text("done"),
        ]);
        let tool_loop = ToolLoop::new(model, tools(vec![transfer])).with_policy(
            AuditedPolicy::new(Arc::clone(&log)).wrapping(Gate {
                asked: Arc::clone(&asked),
            }),
        );
        run_as(&tool_loop, &identity(who)).await.unwrap();
        assert_eq!(used.load(Ordering::SeqCst), runs, "{who}");
        let last = log.export("").await.unwrap().pop().unwrap();
        assert_eq!(last.detail["decision"], decision, "{who}");
        assert_eq!(last.actor.as_ref().map(|a| a.subject.as_str()), Some(who));
    }
    assert_eq!(asked.lock().unwrap().len(), 2, "the inner policy was asked");
}

/// A call the model wrote as text is recorded as such.
#[tokio::test]
async fn a_call_recovered_from_text_is_recorded_as_recovered() {
    let store = Flaky::new();
    let log = required_log(&store);
    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![
        text(r#"{"name": "transfer", "arguments": {}}"#),
        text("done"),
    ]);
    let tool_loop = ToolLoop::new(model, tools(vec![transfer]))
        .with_policy(AuditedPolicy::new(Arc::clone(&log)));
    run_as(&tool_loop, &identity("alice")).await.unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 1);
    let r = log.export("").await.unwrap().pop().unwrap();
    assert_eq!(r.detail["recoveredFromText"], true);
}

/// A denial the store cannot take is not turned into a different answer: the
/// model is told the inner policy's reason, and the record is kept for when
/// the store answers, as for any record of a required log.
#[tokio::test(start_paused = true)]
async fn a_denial_the_store_refuses_is_kept_and_the_reason_is_unchanged() {
    let store = Flaky::new();
    let log = required_log(&store);
    store.set_down(true);
    let (transfer, used) = mutating();
    let (model, seen) = Scripted::new(vec![
        calls(vec![call("c1", "transfer", "{}")]),
        text("done"),
    ]);
    let tool_loop = ToolLoop::new(model, tools(vec![transfer])).with_policy(
        AuditedPolicy::new(Arc::clone(&log)).wrapping(Gate {
            asked: Arc::default(),
        }),
    );
    run_as(&tool_loop, &identity("mallory")).await.unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 0);
    let told = results(&seen.lock().unwrap().clone(), 1);
    assert!(told[0].1.contains("mallory may not transfer"), "{told:?}");
    assert_eq!(
        log.pending_outcomes(),
        1,
        "the denial is waiting for the store"
    );

    store.set_down(false);
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    let r = log.export("").await.unwrap().pop().unwrap();
    assert_eq!(r.detail["decision"], "deny");
}
