// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the parts of the sweep that need no server.
//!
//! `sweep` itself drives eleven methods against a live agent and cannot be
//! unit-tested without one. What *can* be tested here is every decision it
//! makes about what it saw — and those are the parts that decide whether a
//! cell is recorded as covered, which is the number the whole crate exists to
//! make trustworthy.

use a2a_protocol_client::ClientError;
use a2a_protocol_types::error::A2aError;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use super::{is_protocol_refusal, make_send_params, task_id_of};
use crate::tests::transport_failures;

// ── make_send_params ─────────────────────────────────────────────────────────

#[test]
fn make_send_params_carries_one_user_text_part() {
    let p = make_send_params("hello");
    assert_eq!(p.message.role, MessageRole::User);
    assert_eq!(p.message.parts.len(), 1);
    assert!(matches!(
        &p.message.parts[0].content,
        PartContent::Text(t) if t == "hello"
    ));
    // A fresh turn, not a continuation: the sweep's first call must create a
    // task rather than join one.
    assert!(p.message.task_id.is_none());
    assert!(p.message.context_id.is_none());
    assert!(p.tenant.is_none());
}

#[test]
fn every_message_gets_its_own_id() {
    // The sweep sends several messages per binding. Two sharing an id would
    // make the second look like a retransmission of the first.
    let a = make_send_params("x");
    let b = make_send_params("x");
    assert_ne!(a.message.id, b.message.id);
}

// ── is_protocol_refusal ──────────────────────────────────────────────────────

/// This is the load-bearing distinction in the whole sweep. A refusal means
/// the method was routed, parsed and evaluated — which is the thing being
/// measured. A connection error means nothing was exercised at all, so
/// counting one as the other would let a binding that never connected be
/// reported as covered.
#[test]
fn only_a_protocol_answer_counts_as_a_refusal() {
    assert!(is_protocol_refusal(&ClientError::Protocol(
        A2aError::invalid_params("no")
    )));
    for e in transport_failures() {
        assert!(
            !is_protocol_refusal(&e),
            "a transport failure was counted as a protocol refusal: {e}"
        );
    }
}

// ── task_id_of ───────────────────────────────────────────────────────────────

fn task(id: &str) -> Task {
    Task {
        id: TaskId::new(id),
        context_id: ContextId::new("ctx"),
        status: TaskStatus::new(TaskState::Working),
        history: None,
        artifacts: None,
        metadata: None,
    }
}

fn message() -> Message {
    Message {
        id: MessageId::new("m-1"),
        role: MessageRole::Agent,
        parts: vec![Part::text("hi")],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

/// The sweep learns the task id it must later get, cancel and subscribe to
/// from whichever event arrives first, so every event that carries one has to
/// yield it. A variant silently returning `None` would strand the rest of that
/// binding's column.
#[test]
fn task_id_is_read_from_every_event_that_carries_one() {
    assert_eq!(
        task_id_of(&StreamResponse::Task(task("t-task"))),
        Some("t-task".to_string())
    );
    assert_eq!(
        task_id_of(&StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new("t-status"),
            context_id: ContextId::new("ctx"),
            status: TaskStatus::new(TaskState::Working),
            metadata: None,
        })),
        Some("t-status".to_string())
    );
    assert_eq!(
        task_id_of(&StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            task_id: TaskId::new("t-artifact"),
            context_id: ContextId::new("ctx"),
            artifact: a2a_protocol_types::artifact::Artifact::new("a", vec![Part::text("x")]),
            append: None,
            last_chunk: Some(true),
            metadata: None,
        })),
        Some("t-artifact".to_string())
    );
}

/// A bare `Message` is a synchronous answer with no task behind it. Inventing
/// an id for it would make the sweep chase a task that does not exist.
#[test]
fn a_bare_message_carries_no_task_id() {
    assert_eq!(task_id_of(&StreamResponse::Message(message())), None);
}
