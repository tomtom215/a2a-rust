// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Every place [`ContentKeys`] looks is looked at: a client that requires
//! signed content refuses unsigned content wherever a response carries it.
//! An empty key set suffices, because what is refused is the absence of a
//! signature, not a signature that fails to verify.

#![cfg(feature = "signing")]

use a2a_protocol_types::events::TaskStatusUpdateEvent;
use a2a_protocol_types::message::Message;
use a2a_protocol_types::responses::{SendMessageResponse, TaskListResponse};
use a2a_protocol_types::signing::Jwks;
use a2a_protocol_types::task::{Task, TaskState, TaskStatus};

use super::*;
use crate::error::ClientError;

fn requiring() -> ContentKeys {
    ContentKeys::new(Jwks::new(Vec::new())).requiring_signatures()
}

fn unsigned() -> Message {
    Message::user_text("m-1", "unsigned")
}

/// A task whose only unsigned content is its status message.
fn task_with_status_message() -> Task {
    let mut status = TaskStatus::new(TaskState::Working);
    status.message = Some(unsigned());
    Task::new("t-1", "c-1", status)
}

fn refused(result: ClientResult<()>, what: &str) {
    match result {
        Err(ClientError::ContentNotVerified(why)) => {
            assert!(why.contains(what), "{why}");
        }
        other => panic!("expected {what} to be refused, got {other:?}"),
    }
}

#[test]
fn a_streamed_message_is_checked() {
    refused(
        requiring().event(&StreamResponse::Message(unsigned())),
        "message",
    );
}

#[test]
fn a_status_updates_message_is_checked() {
    let mut status = TaskStatus::new(TaskState::Working);
    status.message = Some(unsigned());
    let update = TaskStatusUpdateEvent {
        task_id: "t-1".into(),
        context_id: "c-1".into(),
        status,
        metadata: None,
    };
    refused(
        requiring().event(&StreamResponse::StatusUpdate(update)),
        "status message",
    );
}

#[test]
fn a_streamed_task_is_checked() {
    refused(
        requiring().event(&StreamResponse::Task(task_with_status_message())),
        "status message",
    );
}

#[test]
fn every_task_in_a_page_is_checked() {
    let page = TaskListResponse::new(vec![
        Task::new("t-0", "c-1", TaskStatus::new(TaskState::Completed)),
        task_with_status_message(),
    ]);
    refused(requiring().page(&page), "status message");
}

#[test]
fn both_kinds_of_send_response_are_checked() {
    refused(
        requiring().response(&SendMessageResponse::Task(task_with_status_message())),
        "status message",
    );
    refused(
        requiring().response(&SendMessageResponse::Message(unsigned())),
        "message",
    );
}

/// What does not need a signature is not refused: history, and content
/// when no keys are set.
#[test]
fn history_and_a_client_without_keys_are_not_refused() {
    let mut task = Task::new("t-1", "c-1", TaskStatus::new(TaskState::Completed));
    task.history = Some(vec![unsigned()]);
    assert!(requiring().task(&task).is_ok());
    let none = ContentKeys::default();
    assert!(none.event(&StreamResponse::Message(unsigned())).is_ok());
    assert!(none.task(&task_with_status_message()).is_ok());
}

#[test]
fn the_debug_form_counts_keys_and_says_whether_signatures_are_required() {
    let debug = format!("{:?}", requiring());
    assert!(debug.starts_with("ContentKeys"), "{debug}");
    assert!(debug.contains("keys: Some(0)"), "{debug}");
    assert!(debug.contains("require_signed: true"), "{debug}");
}
