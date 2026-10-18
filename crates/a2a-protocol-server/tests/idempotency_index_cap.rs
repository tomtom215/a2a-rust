// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Release audit F8, through the handler: once the in-memory idempotency
//! index is at `TaskStoreConfig::max_idempotency_keys`, a send with a new key
//! is refused as temporarily unavailable, runs nothing and stores nothing,
//! while a retry of a held key still replays and a send without a key still
//! runs.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_server::store::TaskStoreConfig;
use a2a_protocol_server::{RequestHandlerBuilder, SendMessageResult, agent_executor};
use a2a_protocol_types::Message;
use a2a_protocol_types::idempotency::set_key;
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;

static RUNS: AtomicUsize = AtomicUsize::new(0);

struct Counting;
agent_executor!(Counting, |ctx, queue| async {
    RUNS.fetch_add(1, Ordering::SeqCst);
    a2a_protocol_server::EventEmitter::new(ctx, queue)
        .status(a2a_protocol_types::task::TaskState::Completed)
        .await
});

fn keyed(message_id: &str, key: &str) -> MessageSendParams {
    let mut message = Message::user_text(message_id, "do it");
    set_key(&mut message, key).expect("a valid key");
    MessageSendParams::new(message)
}

fn task_id(result: SendMessageResult) -> String {
    match result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    }
}

#[tokio::test]
async fn a_full_index_refuses_a_new_key_without_running_anything() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Counting)
            .without_agent_card()
            .with_task_store_config(TaskStoreConfig::default().with_max_idempotency_keys(Some(1)))
            .build()
            .expect("handler"),
    );

    let first = task_id(
        handler
            .on_send_message(keyed("m1", "first-key-0123456789"), false, None)
            .await
            .expect("the first keyed send runs"),
    );
    assert_eq!(RUNS.load(Ordering::SeqCst), 1);

    let refused = handler
        .on_send_message(keyed("m2", "second-key-0123456789"), false, None)
        .await
        .expect_err("the index is full");
    // A refusal before any work: 503 / RESOURCE_EXHAUSTED, which a client
    // may retry. Rewritten to INTERNAL by the send path's `not_retry_safe`
    // until the claim mapped it (found by this test).
    assert!(
        matches!(refused, a2a_protocol_server::ServerError::Overloaded(_)),
        "a full index is a temporary refusal: {refused:?}"
    );
    assert!(
        refused.to_string().contains("idempotency index full"),
        "{refused}"
    );
    assert_eq!(RUNS.load(Ordering::SeqCst), 1, "the refused send ran");

    let replay = task_id(
        handler
            .on_send_message(keyed("m1", "first-key-0123456789"), false, None)
            .await
            .expect("a held key still replays"),
    );
    assert_eq!(replay, first);
    assert_eq!(RUNS.load(Ordering::SeqCst), 1, "the replay ran again");

    handler
        .on_send_message(
            MessageSendParams::new(Message::user_text("m3", "no key")),
            false,
            None,
        )
        .await
        .expect("a send without a key is not affected");
    assert_eq!(RUNS.load(Ordering::SeqCst), 2);

    let listed = handler
        .on_list_tasks(Default::default(), None)
        .await
        .expect("list");
    assert_eq!(listed.tasks.len(), 2, "the refused send left a task behind");
}
