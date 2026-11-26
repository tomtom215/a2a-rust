// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::cancel_cause::cause_of;
use a2a_protocol_types::events::TaskStatusUpdateEvent;
use a2a_protocol_types::params::{CancelTaskParams, MessageSendParams, TaskQueryParams};
use a2a_protocol_types::responses::SendMessageResponse;

use super::*;
use crate::builder::RequestHandlerBuilder;
use crate::{EventEmitter, RequestHandler, SendMessageResult, agent_executor};

/// The cause on the stored `Canceled` status of task `id`, read with
/// `GetTask` (COR-04's tests for each cause use it).
pub async fn cause(h: &RequestHandler, tenant: Option<&str>, id: &str) -> Option<CancelCause> {
    let mut q = TaskQueryParams::new(id.to_owned());
    if let Some(t) = tenant {
        q = q.with_tenant(t.to_owned());
    }
    let task = h.on_get_task(q, None).await.expect("get");
    assert_eq!(task.status.state, TaskState::Canceled);
    task.status.message.as_ref().and_then(cause_of)
}

fn update(state: TaskState, message: Option<Message>) -> StreamResponse {
    let mut status = TaskStatus::new(state);
    status.message = message;
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        status,
    ))
}

fn status_of(event: &StreamResponse) -> &TaskStatus {
    match event {
        StreamResponse::StatusUpdate(u) => &u.status,
        _ => unreachable!(),
    }
}

#[test]
fn a_lapsed_lease_is_named_before_the_halt_it_stops_the_turn_with() {
    let turn = ExecutorTurn::default();
    assert_eq!(turn.cancel_cause(), None);
    turn.request_cancel();
    assert_eq!(turn.cancel_cause(), Some(CancelCause::CancelRequested));
    turn.halt();
    assert_eq!(turn.cancel_cause(), Some(CancelCause::Halted));
    turn.lapse_lease();
    assert_eq!(turn.cancel_cause(), Some(CancelCause::LeaseLapsed));
}

#[test]
fn a_canceled_status_without_a_message_is_given_one_naming_the_task() {
    let mut event = update(TaskState::Canceled, None);
    stamp(&mut event, Some(CancelCause::Halted));
    let message = status_of(&event).message.as_ref().expect("a message");
    assert_eq!(cause_of(message), Some(CancelCause::Halted));
    assert_eq!(message.role, MessageRole::Agent);
    assert_eq!(message.task_id, Some(TaskId::new("t")));
    assert_eq!(message.context_id, Some(ContextId::new("c")));
}

#[test]
fn an_executors_own_message_keeps_its_text_and_gains_the_cause() {
    let mut event = update(
        TaskState::Canceled,
        Some(Message::agent_text("m", "released the GPU")),
    );
    stamp(&mut event, Some(CancelCause::CancelRequested));
    let message = status_of(&event).message.as_ref().unwrap();
    assert_eq!(cause_of(message), Some(CancelCause::CancelRequested));
    assert_eq!(message.id, MessageId::new("m"));
}

#[test]
fn only_a_canceled_status_with_a_known_cause_is_stamped() {
    let mut working = update(TaskState::Working, None);
    stamp(&mut working, Some(CancelCause::Halted));
    assert!(status_of(&working).message.is_none());
    let mut unknown = update(TaskState::Canceled, None);
    stamp(&mut unknown, None);
    assert!(status_of(&unknown).message.is_none());
}

struct Sleeper;
agent_executor!(Sleeper, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Working)
        .await?;
    ctx.cancellation_token.cancelled().await;
    Ok(())
});

struct Parks;
agent_executor!(Parks, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::InputRequired)
        .await?;
    Ok(())
});

async fn started(h: &RequestHandler, want: TaskState) -> String {
    let mut p = MessageSendParams::new(Message::new(
        MessageId::new(uuid::Uuid::new_v4().to_string()),
        MessageRole::User,
        vec![Part::text("work")],
    ));
    p.configuration =
        Some(serde_json::from_value(serde_json::json!({ "returnImmediately": true })).unwrap());
    let SendMessageResult::Response(SendMessageResponse::Task(t)) =
        h.on_send_message(p, false, None).await.expect("send")
    else {
        panic!("expected a task")
    };
    let q = TaskQueryParams::new(t.id.0.clone());
    tokio::time::timeout(Duration::from_secs(10), async {
        while h
            .on_get_task(q.clone(), None)
            .await
            .expect("get")
            .status
            .state
            != want
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the task reached the state");
    t.id.0
}

/// COR-04: a `CancelTask` of a running task, and of a task parked between
/// turns, says on the canceled status that a caller asked.
#[tokio::test]
async fn a_cancel_task_says_a_caller_asked() {
    for (h, parked) in [
        (RequestHandlerBuilder::new(Sleeper), TaskState::Working),
        (RequestHandlerBuilder::new(Parks), TaskState::InputRequired),
    ]
    .map(|(b, s)| {
        (
            Arc::new(b.without_agent_card().build().expect("handler")),
            s,
        )
    }) {
        let id = started(&h, parked).await;
        let task = h
            .on_cancel_task(CancelTaskParams::new(id.clone()), None)
            .await
            .expect("cancel");
        let answered = task.status.message.as_ref().and_then(cause_of);
        assert_eq!(answered, Some(CancelCause::CancelRequested), "{parked:?}");
        assert_eq!(
            cause(&h, None, &id).await,
            Some(CancelCause::CancelRequested),
            "{parked:?}"
        );
    }
}

/// A whole task in `Canceled`, as the default hook's answer can be, is
/// stamped as a status update is.
#[test]
fn a_canceled_task_event_is_stamped_too() {
    let mut task =
        a2a_protocol_types::task::Task::new("t", "c", TaskStatus::new(TaskState::Canceled));
    task.status.message = None;
    let mut event = StreamResponse::Task(task);
    stamp(&mut event, Some(CancelCause::LeaseLapsed));
    let StreamResponse::Task(task) = &event else {
        unreachable!()
    };
    let message = task.status.message.as_ref().expect("a message");
    assert_eq!(cause_of(message), Some(CancelCause::LeaseLapsed));
    assert_eq!(message.task_id, Some(TaskId::new("t")));
}

/// The message a status is given says its own cause in words, for a reader
/// that does not know the extension.
#[test]
fn the_message_given_says_the_cause() {
    let text = |cause| {
        let mut event = update(TaskState::Canceled, None);
        stamp(&mut event, Some(cause));
        status_of(&event)
            .message
            .as_ref()
            .unwrap()
            .text()
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(
        text(CancelCause::Halted),
        "canceled: an operator halted this work"
    );
    assert_eq!(
        text(CancelCause::LeaseLapsed),
        "canceled: the task's lease was not renewed in time"
    );
    assert_eq!(
        text(CancelCause::CancelRequested),
        "canceled: a caller asked for it"
    );
}
