// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The turn bound, calls written as text, and calls with no id.

use super::*;

#[tokio::test]
async fn a_model_that_never_stops_calling_tools_hits_the_turn_bound() {
    let (status, used) = status_tool();
    let (model, seen) = Scripted::forever(ModelTurn::new(
        "",
        vec![call("c", "service_status", r#"{"service":"checkout"}"#)],
    ));
    let tool_loop = ToolLoop::new(model, tools(vec![status])).with_max_turns(3);
    assert_eq!(run(&tool_loop).await.unwrap_err(), LoopError::TurnLimit(3));
    assert_eq!(seen.lock().unwrap().len(), 3);
    assert_eq!(used.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn the_default_bound_is_eight_turns_and_zero_means_one() {
    let (model, seen) = Scripted::forever(ModelTurn::new("", vec![call("c", "nope", "{}")]));
    let tool_loop = ToolLoop::new(model, ToolSet::new());
    assert_eq!(tool_loop.max_turns(), DEFAULT_MAX_TURNS);
    assert_eq!(DEFAULT_MAX_TURNS, 8);
    assert_eq!(run(&tool_loop).await.unwrap_err(), LoopError::TurnLimit(8));
    assert_eq!(seen.lock().unwrap().len(), 8);

    let (model, seen) = Scripted::forever(ModelTurn::new("", vec![call("c", "nope", "{}")]));
    let tool_loop = ToolLoop::new(model, ToolSet::new()).with_max_turns(0);
    assert_eq!(run(&tool_loop).await.unwrap_err(), LoopError::TurnLimit(1));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_call_written_as_text_is_run_bare_fenced_or_tagged() {
    for written in [
        r#"{"name": "service_status", "arguments": {"service": "checkout"}}"#,
        "```json\n{\"name\": \"service_status\", \"arguments\": {\"service\": \"checkout\"}}\n```",
        "<tool_call>\n{\"name\": \"service_status\", \"arguments\": {\"service\": \"checkout\"}}\n</tool_call>",
    ] {
        let (status, used) = status_tool();
        let (model, seen) = Scripted::new(vec![text(written), text("checkout is healthy")]);
        let answer = run(&ToolLoop::new(model, tools(vec![status])))
            .await
            .unwrap();
        assert_eq!(answer.text, "checkout is healthy", "{written}");
        assert_eq!(used.load(Ordering::SeqCst), 1, "{written}");
        assert_eq!(
            answer.trace,
            [r#"service_status({"service":"checkout"}) -> {"state":"healthy"}"#]
        );
        // Kept in the conversation as the call it was, not as text.
        let seen = seen.lock().unwrap().clone();
        assert_eq!(
            seen[1].messages[1],
            ChatMessage::Assistant {
                text: String::new(),
                tool_calls: vec![call(
                    "recovered-0",
                    "service_status",
                    r#"{"service":"checkout"}"#
                )],
            }
        );
        assert_eq!(results(&seen, 1)[0].0, "recovered-0");
    }
}

#[tokio::test]
async fn text_that_only_contains_json_is_an_answer() {
    for written in [
        r#"Call {"name": "service_status", "arguments": {"service": "checkout"}} next."#,
        r#"{"name": "restart", "arguments": {}}"#,
        r#"{"service": "checkout", "state": "healthy"}"#,
    ] {
        let (status, used) = status_tool();
        let (model, seen) = Scripted::new(vec![text(written)]);
        let answer = run(&ToolLoop::new(model, tools(vec![status])))
            .await
            .unwrap();
        assert_eq!(answer.text, written);
        assert_eq!(used.load(Ordering::SeqCst), 0);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn with_recovery_off_a_call_written_as_text_is_the_answer() {
    let written = r#"{"name": "service_status", "arguments": {"service": "checkout"}}"#;
    let (status, used) = status_tool();
    let (model, _) = Scripted::new(vec![text(written)]);
    let tool_loop = ToolLoop::new(model, tools(vec![status])).with_recover_text_calls(false);
    assert_eq!(run(&tool_loop).await.unwrap().text, written);
    assert_eq!(used.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn calls_without_an_id_are_given_one_the_result_carries() {
    let (status, _) = status_tool();
    let (model, seen) = Scripted::new(vec![
        calls(vec![
            call("", "service_status", r#"{"service":"checkout"}"#),
            call("", "service_status", r#"{"service":"checkout"}"#),
        ]),
        text("done"),
    ]);
    run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap();
    let seen = seen.lock().unwrap().clone();
    let ids: Vec<String> = results(&seen, 1).into_iter().map(|(id, _)| id).collect();
    assert_eq!(ids, ["call-0-0", "call-0-1"]);
}
