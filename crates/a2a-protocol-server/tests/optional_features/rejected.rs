// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! a2a.proto `TASK_STATE_REJECTED`: "This may be done during initial task
//! creation or later once an agent has determined it can't or won't
//! proceed." An executor may reject at once, or after it started working,
//! and the task ends rejected either way.

use a2a_protocol_server::{
    EventEmitter, RequestHandler, RequestHandlerBuilder, SendMessageResult, agent_executor,
};
use a2a_protocol_types::Message;
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;

struct RejectsAtOnce;
agent_executor!(RejectsAtOnce, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Rejected)
        .await
});

struct RejectsLater;
agent_executor!(RejectsLater, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    emit.status(TaskState::Rejected).await
});

/// Sends one message and returns the state the response and the store give.
async fn final_states(handler: RequestHandler) -> (TaskState, TaskState) {
    let sent = handler
        .on_send_message(
            MessageSendParams::new(Message::user_text("m-1", "please")),
            false,
            None,
        )
        .await
        .unwrap();
    let SendMessageResult::Response(SendMessageResponse::Task(task)) = sent else {
        panic!("expected a task, got {sent:?}");
    };
    let stored = handler
        .on_get_task(
            TaskQueryParams {
                tenant: None,
                id: task.id.0.clone(),
                history_length: None,
            },
            None,
        )
        .await
        .unwrap();
    (task.status.state, stored.status.state)
}

#[tokio::test]
async fn an_agent_may_reject_a_task_at_creation() {
    let handler = RequestHandlerBuilder::new(RejectsAtOnce)
        .without_agent_card()
        .build()
        .unwrap();
    assert_eq!(
        final_states(handler).await,
        (TaskState::Rejected, TaskState::Rejected)
    );
}

#[tokio::test]
async fn an_agent_may_reject_a_task_after_it_started_working() {
    let handler = RequestHandlerBuilder::new(RejectsLater)
        .without_agent_card()
        .build()
        .unwrap();
    assert_eq!(
        final_states(handler).await,
        (TaskState::Rejected, TaskState::Rejected)
    );
}
