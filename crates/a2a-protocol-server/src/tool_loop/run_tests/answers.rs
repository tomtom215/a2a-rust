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
