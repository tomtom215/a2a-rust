// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What each kind of reply and tool outcome does to a run.

use serde_json::json;

use super::*;

#[tokio::test]
async fn an_answer_without_tools_is_returned_with_an_empty_trace() {
    let (status, used) = status_tool();
    let (model, seen) = Scripted::new(vec![text("All good.")]);
    let tool_loop = ToolLoop::new(model, tools(vec![status])).with_preamble("Be brief.");
    let answer = run(&tool_loop).await.unwrap();
    assert_eq!(answer.text, "All good.");
    assert_eq!(answer.trace, Vec::<String>::new());
    assert_eq!(used.load(Ordering::SeqCst), 0);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].preamble, "Be brief.");
    assert_eq!(
        seen[0].messages,
        [ChatMessage::User("how is checkout?".to_owned())]
    );
    assert_eq!(seen[0].tools.len(), 1);
    assert_eq!(seen[0].tools[0].name, "service_status");
    assert_eq!(seen[0].tools[0].description, "a test tool");
}

#[tokio::test]
async fn one_call_then_an_answer_threads_the_result_back_and_traces_it() {
    let (status, used) = status_tool();
    let (model, seen) = Scripted::new(vec![
        calls(vec![call(
            "c1",
            "service_status",
            r#"{"service":"checkout"}"#,
        )]),
        text("checkout is healthy"),
    ]);
    let answer = run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap();
    assert_eq!(answer.text, "checkout is healthy");
    assert_eq!(
        answer.trace,
        [r#"service_status({"service":"checkout"}) -> {"state":"healthy"}"#]
    );
    assert_eq!(used.load(Ordering::SeqCst), 1);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    // The second turn carries the whole conversation, the catalogue again,
    // and the result under the call's id.
    assert_eq!(seen[1].messages.len(), 3);
    assert_eq!(
        seen[1].messages[1],
        ChatMessage::Assistant {
            text: String::new(),
            tool_calls: vec![call("c1", "service_status", r#"{"service":"checkout"}"#)],
        }
    );
    assert_eq!(
        results(&seen, 1),
        [("c1".to_owned(), r#"{"state":"healthy"}"#.to_owned())]
    );
    assert_eq!(seen[1].tools.len(), 1);
}

#[tokio::test]
async fn a_failed_tool_goes_back_to_the_model_which_recovers() {
    let (status, used) = status_tool();
    let (model, seen) = Scripted::new(vec![
        calls(vec![call(
            "c1",
            "service_status",
            r#"{"service":"billing"}"#,
        )]),
        calls(vec![call(
            "c2",
            "service_status",
            r#"{"service":"checkout"}"#,
        )]),
        text("checkout is healthy; there is no billing"),
    ]);
    let answer = run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap();
    assert_eq!(answer.text, "checkout is healthy; there is no billing");
    assert_eq!(answer.trace.len(), 2);
    assert_eq!(
        answer.trace[0],
        r#"service_status({"service":"billing"}) -> error: no service named 'billing'; call list_services"#
    );
    assert_eq!(used.load(Ordering::SeqCst), 2);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        results(&seen, 1),
        [(
            "c1".to_owned(),
            "error: no service named 'billing'; call list_services".to_owned()
        )]
    );
}

#[tokio::test]
async fn an_unknown_tool_is_answered_with_the_catalogue() {
    let (status, used) = status_tool();
    let (model, seen) = Scripted::new(vec![
        calls(vec![call("c1", "status_of", r#"{"service":"checkout"}"#)]),
        text("sorry"),
    ]);
    let answer = run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap();
    assert_eq!(answer.text, "sorry");
    assert_eq!(used.load(Ordering::SeqCst), 0);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        results(&seen, 1),
        [(
            "c1".to_owned(),
            "error: no tool named 'status_of'; the tools are: service_status".to_owned()
        )]
    );
    assert!(
        answer.trace[0].starts_with("status_of("),
        "{:?}",
        answer.trace
    );
}

#[tokio::test]
async fn arguments_that_are_not_a_json_object_go_back_without_calling_the_tool() {
    let (status, used) = status_tool();
    let (model, seen) = Scripted::new(vec![
        calls(vec![
            call("c1", "service_status", r#"{"service": "checkout""#),
            call("c2", "service_status", r#"["checkout"]"#),
        ]),
        text("giving up"),
    ]);
    let answer = run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap();
    assert_eq!(answer.text, "giving up");
    assert_eq!(used.load(Ordering::SeqCst), 0, "the tool never ran");
    let seen = seen.lock().unwrap().clone();
    let results = results(&seen, 1);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "c1");
    assert!(
        results[0]
            .1
            .starts_with("error: the arguments are not valid JSON"),
        "{results:?}"
    );
    assert_eq!(
        results[1],
        (
            "c2".to_owned(),
            "error: the arguments must be a JSON object".to_owned()
        )
    );
}

#[tokio::test]
async fn empty_arguments_are_an_empty_object() {
    let (tool, used) = TestTool::new("list_services", |args| {
        assert_eq!(args, json!({}));
        Ok(r#"["checkout"]"#.to_owned())
    });
    let (model, _) = Scripted::new(vec![
        calls(vec![call("c1", "list_services", "  ")]),
        text("checkout"),
    ]);
    run(&ToolLoop::new(model, tools(vec![tool]))).await.unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_fatal_tool_error_ends_the_run_and_later_calls_do_not_run() {
    let (fatal, _) = TestTool::new("deploy", |_| {
        Err(ToolError::Fatal("the deploy API is down".to_owned()))
    });
    let (status, used) = status_tool();
    let (model, seen) = Scripted::new(vec![
        calls(vec![
            call("c1", "deploy", "{}"),
            call("c2", "service_status", r#"{"service":"checkout"}"#),
        ]),
        text("never asked"),
    ]);
    let err = run(&ToolLoop::new(model, tools(vec![fatal, status])))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        LoopError::Fatal {
            tool: "deploy".to_owned(),
            message: "the deploy API is down".to_owned()
        }
    );
    assert_eq!(used.load(Ordering::SeqCst), 0);
    assert_eq!(seen.lock().unwrap().len(), 1, "no turn after a fatal error");
}

#[tokio::test]
async fn a_provider_failure_ends_the_run_as_the_providers() {
    let failure = ModelError::Status {
        status: 500,
        body: "Failed to parse tool call arguments as JSON".to_owned(),
    };
    let (model, _) = Scripted::new(vec![Err(failure.clone())]);
    let err = run(&ToolLoop::new(model, ToolSet::new()))
        .await
        .unwrap_err();
    assert_eq!(err, LoopError::Model(failure));
    assert!(err.to_string().contains("HTTP 500"), "{err}");
}
