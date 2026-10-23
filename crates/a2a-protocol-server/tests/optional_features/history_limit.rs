// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! a2a.proto `SendMessageConfiguration.history_length` and
//! `GetTaskRequest.history_length`: the server returns at most that many
//! recent messages, "but MAY apply a lower limit". This server keeps at most
//! 1,024 messages per task (`MAX_TASK_HISTORY_MESSAGES` in
//! `handler/messaging/decisions.rs`), so a request for more than that gets
//! that many, the newest, even when the conversation holds more.

use std::sync::Arc;

use a2a_protocol_server::{
    EventEmitter, InMemoryTaskStore, RequestHandler, RequestHandlerBuilder, SendMessageResult,
    TaskStore, agent_executor,
};
use a2a_protocol_types::Message;
use a2a_protocol_types::params::{MessageSendParams, SendMessageConfiguration, TaskQueryParams};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

/// The server's per-task history limit, which the crate does not export.
const MAX_TASK_HISTORY_MESSAGES: usize = 1024;

/// Asks for more input every turn, so the task stays open for the next.
struct AsksAgain;
agent_executor!(AsksAgain, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::InputRequired)
        .await
});

/// A handler whose store holds task `long` with a full history.
async fn handler_with_a_full_history() -> RequestHandler {
    let store = Arc::new(InMemoryTaskStore::new());
    let history: Vec<Message> = (0..MAX_TASK_HISTORY_MESSAGES)
        .map(|i| Message::user_text(format!("old-{i}"), "earlier turn"))
        .collect();
    store
        .save(&Task {
            id: TaskId::new("long"),
            context_id: ContextId::new("ctx-long"),
            status: TaskStatus::new(TaskState::InputRequired),
            history: Some(history),
            artifacts: None,
            metadata: None,
        })
        .await
        .unwrap();
    RequestHandlerBuilder::new(AsksAgain)
        .without_agent_card()
        .with_task_store_arc(store)
        .build()
        .unwrap()
}

/// One more turn on task `long`, asking for `history_length` messages back.
async fn one_more_turn(handler: &RequestHandler, history_length: u32) -> Task {
    let mut message = Message::user_text("newest", "one more turn");
    message.task_id = Some(TaskId::new("long"));
    let mut params = MessageSendParams::new(message);
    params.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: vec![],
        task_push_notification_config: None,
        history_length: Some(history_length),
        return_immediately: None,
    });
    match handler.on_send_message(params, false, None).await.unwrap() {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => task,
        other => panic!("expected a task, got {other:?}"),
    }
}

fn ids(task: &Task) -> Vec<String> {
    task.history
        .iter()
        .flatten()
        .map(|m| m.id.0.clone())
        .collect()
}

#[tokio::test]
async fn send_message_history_length_is_capped_below_what_was_asked() {
    let handler = handler_with_a_full_history().await;
    let asked = u32::try_from(MAX_TASK_HISTORY_MESSAGES).unwrap() * 2;
    let task = one_more_turn(&handler, asked).await;

    // The conversation now holds MAX + 1 messages and the caller asked for
    // 2 * MAX; the server's own limit is the lower one.
    let history = ids(&task);
    assert_eq!(history.len(), MAX_TASK_HISTORY_MESSAGES);
    assert_eq!(history.last().map(String::as_str), Some("newest"));
    assert_eq!(history[0], "old-1", "the oldest message is the one dropped");

    // The control: below the limit, historyLength is what is returned.
    let task = one_more_turn(&handler, 3).await;
    assert_eq!(ids(&task).len(), 3);
}

#[tokio::test]
async fn get_task_history_length_is_capped_below_what_was_asked() {
    let handler = handler_with_a_full_history().await;
    one_more_turn(&handler, 1).await;

    let asked = u32::try_from(MAX_TASK_HISTORY_MESSAGES).unwrap() * 2;
    let get = |history_length| TaskQueryParams {
        tenant: None,
        id: "long".into(),
        history_length: Some(history_length),
    };
    let task = handler.on_get_task(get(asked), None).await.unwrap();
    let history = ids(&task);
    assert_eq!(history.len(), MAX_TASK_HISTORY_MESSAGES);
    assert_eq!(history.last().map(String::as_str), Some("newest"));

    let task = handler.on_get_task(get(2), None).await.unwrap();
    assert_eq!(
        ids(&task).len(),
        2,
        "the control: a smaller length is honoured"
    );
}
