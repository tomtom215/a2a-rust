// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the final reply makes of a run: an answer with the reason it
//! stopped, or no answer (audit M-1).

use super::*;

#[allow(clippy::unnecessary_wraps)] // a scripted reply, like the `Err` ones beside it
fn stopped(text: &str, reason: &str) -> Result<ModelTurn, ModelError> {
    Ok(ModelTurn::new(text, Vec::new()).with_finish_reason(reason))
}

fn empty(reason: Option<&str>) -> LoopError {
    LoopError::EmptyAnswer {
        finish_reason: reason.map(str::to_owned),
    }
}

/// The reason is the final reply's own: an earlier turn's (`tool_calls`)
/// does not stand in for it, and a reply with none has none.
#[tokio::test]
async fn the_answer_carries_the_final_replys_finish_reason() {
    let (status, _) = status_tool();
    let asked = Ok(ModelTurn::new(
        "",
        vec![call("c1", "service_status", r#"{"service":"checkout"}"#)],
    )
    .with_finish_reason("tool_calls"));
    let (model, _) = Scripted::new(vec![asked, stopped("healthy", "stop")]);
    let answer = run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap();
    assert_eq!(answer.finish_reason.as_deref(), Some("stop"));
    assert!(!answer.truncated());

    let (model, _) = Scripted::new(vec![text("no reason given")]);
    let answer = run(&ToolLoop::new(model, ToolSet::new())).await.unwrap();
    assert_eq!(answer.finish_reason, None);
}

/// A reply cut at the output limit is an answer, marked as one that was cut.
#[tokio::test]
async fn a_reply_cut_at_the_output_limit_is_an_answer_that_says_so() {
    let (model, _) = Scripted::new(vec![stopped("checkout is hea", "length")]);
    let answer = run(&ToolLoop::new(model, ToolSet::new())).await.unwrap();
    assert_eq!(answer.text, "checkout is hea");
    assert!(answer.truncated());
}

/// No text, or only whitespace, is no answer, with the reason the model gave.
#[tokio::test]
async fn a_final_reply_without_text_is_no_answer() {
    for (reply, expected) in [
        (text(""), empty(None)),
        (stopped("", "length"), empty(Some("length"))),
        (stopped(" \n\t", "stop"), empty(Some("stop"))),
    ] {
        let (model, seen) = Scripted::new(vec![reply]);
        let error = run(&ToolLoop::new(model, ToolSet::new()))
            .await
            .unwrap_err();
        assert_eq!(error, expected);
        assert_eq!(seen.lock().unwrap().len(), 1, "no second turn is asked for");
    }
}

/// The tools ran, and the model then said nothing: still no answer, and the
/// run is recorded as one that failed that way.
#[tokio::test]
async fn an_empty_reply_after_a_tool_call_is_no_answer() {
    let (status, used) = status_tool();
    let (model, _) = Scripted::new(vec![
        calls(vec![call(
            "c1",
            "service_status",
            r#"{"service":"checkout"}"#,
        )]),
        stopped("", "length"),
    ]);
    let error = run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap_err();
    assert_eq!(error, empty(Some("length")));
    assert_eq!(used.load(Ordering::SeqCst), 1);
}

/// A call written as text is run as a call, and the answer's reason is that
/// of the reply after it, not of the turn that wrote the call.
#[tokio::test]
async fn a_recovered_call_leaves_the_reason_to_the_reply_after_it() {
    let (status, _) = status_tool();
    let written = stopped(
        r#"{"name": "service_status", "arguments": {"service": "checkout"}}"#,
        "length",
    );
    let (model, _) = Scripted::new(vec![written, stopped("healthy", "stop")]);
    let answer = run(&ToolLoop::new(model, tools(vec![status])))
        .await
        .unwrap();
    assert_eq!(answer.text, "healthy");
    assert_eq!(answer.finish_reason.as_deref(), Some("stop"));
}

/// A reply with more calls than the bound runs the first ones and drops the
/// rest, and the conversation holds only what ran (hostile campaign, wave 2,
/// W2-LIVE-5: 90,000 calls in a reply took the loop to 1.4 GB).
#[tokio::test]
async fn a_reply_with_more_calls_than_the_bound_runs_the_first_ones() {
    let (status, used) = status_tool();
    let five: Vec<ToolCall> = (0..5)
        .map(|i| {
            call(
                &format!("c{i}"),
                "service_status",
                r#"{"service":"checkout"}"#,
            )
        })
        .collect();
    let (model, seen) = Scripted::new(vec![calls(five), text("done")]);
    let tool_loop = ToolLoop::new(model, tools(vec![status])).with_max_calls_per_turn(2);
    let answer = run(&tool_loop).await.unwrap();
    assert_eq!(answer.text, "done");
    assert_eq!(used.load(Ordering::SeqCst), 2, "only the first two ran");
    assert_eq!(answer.trace.len(), 3, "{:?}", answer.trace);
    // The note comes first, as the calls that follow it are the ones it names.
    assert_eq!(
        answer.trace[0],
        "(a reply asked for 5 tool calls; the first 2 ran)"
    );
    assert!(
        answer.trace[1].starts_with("service_status("),
        "{:?}",
        answer.trace
    );
    let seen = seen.lock().unwrap().clone();
    let ChatMessage::Assistant { tool_calls, .. } = &seen[1].messages[1] else {
        panic!("the assistant turn");
    };
    assert_eq!(
        tool_calls.len(),
        2,
        "the dropped calls are not in the history"
    );
    assert_eq!(results(&seen, 1).len(), 2);
}

/// Exactly the bound is not over it; the default and the floor are as documented.
#[tokio::test]
async fn the_bound_on_calls_per_reply_has_a_default_and_a_floor() {
    let (status, used) = status_tool();
    let two = vec![
        call("a", "service_status", r#"{"service":"checkout"}"#),
        call("b", "service_status", r#"{"service":"checkout"}"#),
    ];
    let (model, _) = Scripted::new(vec![calls(two), text("done")]);
    let tool_loop = ToolLoop::new(model, tools(vec![status])).with_max_calls_per_turn(2);
    let answer = run(&tool_loop).await.unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 2);
    assert_eq!(answer.trace.len(), 2, "no note when nothing was dropped");

    let (model, _) = Scripted::new(vec![]);
    let default = ToolLoop::new(model, ToolSet::new());
    assert_eq!(default.max_calls_per_turn(), DEFAULT_MAX_CALLS_PER_TURN);
    assert_eq!(DEFAULT_MAX_CALLS_PER_TURN, 64);
    assert_eq!(default.with_max_calls_per_turn(0).max_calls_per_turn(), 1);
}

/// A result of a megabyte goes to the model whole and into the trace as a
/// bounded line (hostile campaign, wave 2, W2-LIVE-4).
#[tokio::test]
async fn a_large_result_reaches_the_model_whole_and_the_trace_bounded() {
    let (big, _) = TestTool::new("big", |_| Ok("z".repeat(1 << 20)));
    let (model, seen) = Scripted::new(vec![calls(vec![call("c1", "big", "{}")]), text("done")]);
    let answer = run(&ToolLoop::new(model, tools(vec![big]))).await.unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        results(&seen, 1)[0].1.len(),
        1 << 20,
        "the model saw all of it"
    );
    assert_eq!(answer.trace.len(), 1);
    let line = &answer.trace[0];
    assert!(line.starts_with("big({}) -> zzzz"), "{}", &line[..40]);
    assert!(
        line.len() < crate::tool_loop::TRACE_LINE_LIMIT + 40,
        "{}",
        line.len()
    );
    assert!(
        line.ends_with("more bytes]"),
        "{}",
        &line[line.len() - 30..]
    );
}
