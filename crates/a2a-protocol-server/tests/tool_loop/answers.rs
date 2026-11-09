// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The task a run ends in when the model's last reply is empty, or cut at
//! its output limit (audit M-1): the adopter's walk-through had both
//! `COMPLETED`, the second with no mark that the text was a prefix.

use a2a_protocol_server::tool_loop::{
    ANSWER_ARTIFACT_ID, FINISH_REASON_METADATA_KEY, ModelError, ModelTurn,
};
use a2a_protocol_types::failure::FailureClass;
use a2a_protocol_types::task::TaskState;
use serde_json::json;

use super::asks;
use super::executor::{blocking, handler, reason, scripted, streamed};

fn stopped(text: &str, finish_reason: &str) -> Result<ModelTurn, ModelError> {
    Ok(ModelTurn::new(text, Vec::new()).with_finish_reason(finish_reason))
}

/// An empty reply fails the task, with a class that says what to do next and
/// no artifact; the reason the provider gave reaches the caller only as one
/// of the two this crate knows.
#[tokio::test]
async fn an_empty_answer_fails_the_task_with_a_class_for_its_cause() {
    for (reply, class, told) in [
        (
            stopped("", "stop"),
            FailureClass::Internal,
            "the model returned an empty answer",
        ),
        (
            stopped("  \n", "account acme is over quota"),
            FailureClass::Internal,
            "the model returned an empty answer",
        ),
        (
            Ok(ModelTurn::new("", Vec::new())),
            FailureClass::Internal,
            "the model returned an empty answer",
        ),
        (
            stopped("", "length"),
            FailureClass::BudgetExhausted,
            "the model stopped at its output limit before it wrote any text",
        ),
        (
            stopped("", "content_filter"),
            FailureClass::PolicyRefusal,
            "the model provider withheld the answer (content filter)",
        ),
    ] {
        let task = blocking(&handler(scripted(vec![reply])), "go").await;
        assert_eq!(task.status.state, TaskState::Failed, "{told}");
        assert_eq!(task.failure_class(), Some(class), "{told}");
        assert_eq!(reason(&task), told);
        assert!(task.artifacts.unwrap_or_default().is_empty(), "{told}");
    }
}

/// A reply cut at the output limit is a completed task whose answer
/// artifact and status message both say so, and whose text is whole.
#[tokio::test]
async fn a_reply_cut_at_the_output_limit_completes_and_says_so() {
    let task = blocking(
        &handler(scripted(vec![stopped("checkout is hea", "length")])),
        "go",
    )
    .await;
    assert_eq!(task.status.state, TaskState::Completed);
    let marker = json!({ FINISH_REASON_METADATA_KEY: "length" });
    let artifacts = task.artifacts.unwrap_or_default();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id.0, ANSWER_ARTIFACT_ID);
    assert_eq!(
        artifacts[0].parts[0].text_content(),
        Some("checkout is hea")
    );
    assert_eq!(artifacts[0].metadata.as_ref(), Some(&marker));
    let note = task.status.message.as_ref().expect("a status message");
    assert_eq!(
        note.text(),
        Some("The answer was cut off at the model's output limit and may be incomplete.")
    );
    assert_eq!(note.metadata.as_ref(), Some(&marker));
    assert_eq!(note.task_id.as_ref(), Some(&task.id));
    assert_eq!(note.context_id.as_ref(), Some(&task.context_id));
}

/// An answer that ended by itself, or whose provider gave no reason, carries
/// no mark and no status message.
#[tokio::test]
async fn an_answer_that_ended_by_itself_carries_no_mark() {
    for reply in [
        stopped("all good", "stop"),
        Ok(ModelTurn::new("all good", Vec::new())),
    ] {
        let task = blocking(&handler(scripted(vec![reply])), "go").await;
        assert_eq!(task.status.state, TaskState::Completed);
        let artifacts = task.artifacts.unwrap_or_default();
        assert_eq!(artifacts[0].parts[0].text_content(), Some("all good"));
        assert_eq!(artifacts[0].metadata, None);
        assert!(task.status.message.is_none(), "{:?}", task.status.message);
    }
}

/// Streamed, the order is as before: the answer, then `Completed`.
#[tokio::test]
async fn a_cut_off_answer_streams_the_answer_then_completed() {
    let events = streamed(&handler(scripted(vec![stopped(
        "checkout is hea",
        "length",
    )])))
    .await;
    assert_eq!(
        events,
        [
            format!("task {}", TaskState::Submitted),
            format!("status {}", TaskState::Working),
            format!("artifact {ANSWER_ARTIFACT_ID} checkout is hea"),
            format!("status {}", TaskState::Completed),
        ]
    );
}

#[test]
fn the_mark_is_namespaced_like_the_other_extensions_keys() {
    assert_eq!(FINISH_REASON_METADATA_KEY, "a2a-rust.com/finish-reason");
}

/// A run that fails after a tool ran still sends the trace, before the
/// failure: the call has run, and a caller that cannot see it retries with new
/// idempotency keys and runs it again. Before M-1 the empty reply completed
/// the task with this trace; a run that hit the turn bound never had one
/// (hostile campaign, wave 2, W2-LIVE-3).
#[tokio::test]
async fn a_failed_run_sends_the_trace_of_the_calls_it_made() {
    use a2a_protocol_server::tool_loop::TRACE_ARTIFACT_ID;
    let trace_of = |task: &a2a_protocol_types::task::Task| -> Option<String> {
        task.artifacts
            .as_ref()?
            .iter()
            .find(|a| a.id.0 == TRACE_ARTIFACT_ID)?
            .parts
            .first()?
            .text_content()
            .map(str::to_owned)
    };
    let call = r#"service_status({"service":"checkout"}) -> {"state":"healthy"}"#;

    // An empty final reply after a call.
    let task = blocking(
        &handler(scripted(vec![
            asks("service_status", r#"{"service":"checkout"}"#),
            stopped("", "stop"),
        ])),
        "go",
    )
    .await;
    assert_eq!(task.status.state, TaskState::Failed);
    assert_eq!(trace_of(&task).as_deref(), Some(call));

    // The turn bound.
    let looping =
        scripted(vec![asks("service_status", r#"{"service":"checkout"}"#)]).with_max_turns(2);
    let task = blocking(&handler(looping), "go").await;
    assert_eq!(task.status.state, TaskState::Failed);
    assert_eq!(task.failure_class(), Some(FailureClass::BudgetExhausted));
    assert_eq!(trace_of(&task), Some(format!("{call}\n{call}")));

    // Nothing ran: nothing to send.
    let task = blocking(&handler(scripted(vec![stopped("", "stop")])), "go").await;
    assert_eq!(task.status.state, TaskState::Failed);
    assert!(task.artifacts.unwrap_or_default().is_empty());
}
