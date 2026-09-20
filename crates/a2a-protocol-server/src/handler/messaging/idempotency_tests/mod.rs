// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! End-to-end tests for idempotency keys on the send path.
//!
//! The store's own tests cover the claim in isolation. These cover what a
//! caller actually observes: that a retried send returns the first task and
//! runs the executor exactly once, that a reused key is refused rather than
//! answered, and that a send failing after the claim leaves the key reusable.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_types::idempotency::IDEMPOTENCY_METADATA_KEY;

use super::*;

mod fixtures;
mod replay_wait;
use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::streaming::EventQueueReader as _;
use fixtures::{
    CountingExecutor, FailFirstSaveStore, NoIdempotencyStore, advertised, card, counting_handler,
    keyed_params, task_of,
};

const KEY: &str = "8f14e45fceea167a5a36dedd4bea2543";
const OTHER_KEY: &str = "0123456789abcdef0123456789abcdef";

// ── The behaviour the key exists for ─────────────────────────────────────

#[tokio::test]
async fn a_retry_of_the_same_message_returns_the_first_task_and_runs_the_agent_once() {
    // The case the feature exists for: the first send's connection dropped
    // after the bytes went out, so the caller resends the identical message.
    let (handler, executions) = counting_handler();

    let first = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect("first send should succeed");
    let first_id = task_of(&first).id.clone();

    let second = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect("a retry of the identical message should succeed");

    assert_eq!(
        task_of(&second).id,
        first_id,
        "a retry must return the task the first send created"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "the agent must run exactly once across a send and its retry"
    );
}

#[tokio::test]
async fn many_retries_still_run_the_agent_once() {
    let (handler, executions) = counting_handler();
    let first = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .unwrap();
    let first_id = task_of(&first).id.clone();

    for attempt in 0..5 {
        let again = handler
            .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
            .await
            .unwrap_or_else(|e| panic!("retry {attempt} failed: {e}"));
        assert_eq!(task_of(&again).id, first_id);
    }
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concurrent_duplicates_run_the_agent_once() {
    // Two in-flight copies of one send — the shape a caller produces when it
    // retries before the first attempt has answered.
    let (handler, executions) = counting_handler();
    let handler = Arc::new(handler);

    let mut sends = Vec::new();
    for _ in 0..16 {
        let handler = Arc::clone(&handler);
        sends.push(tokio::spawn(async move {
            handler
                .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
                .await
        }));
    }

    let mut ids = Vec::new();
    for send in sends {
        let result = send.await.unwrap().expect("no duplicate should be refused");
        ids.push(task_of(&result).id.clone());
    }

    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "agent ran more than once"
    );
    assert!(
        ids.windows(2).all(|w| w[0] == w[1]),
        "every duplicate must name the same task, got {ids:?}"
    );
}

// ── A reused key is refused, never answered ──────────────────────────────

#[tokio::test]
async fn a_different_message_reusing_the_key_is_refused() {
    // Returning the first task here would answer a message that was never
    // sent, and the caller would act on it.
    let (handler, executions) = counting_handler();
    handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .unwrap();

    let err = handler
        .on_send_message(keyed_params("msg-2", Some(KEY)), false, None)
        .await
        .expect_err("a reused key must be refused");

    assert!(
        matches!(err, ServerError::InvalidParams(ref m) if m.contains("already held")),
        "unhelpful error: {err}"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "the refused send must not have run"
    );
}

#[tokio::test]
async fn distinct_keys_are_independent_sends() {
    let (handler, executions) = counting_handler();
    let a = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .unwrap();
    let b = handler
        .on_send_message(keyed_params("msg-2", Some(OTHER_KEY)), false, None)
        .await
        .unwrap();

    assert_ne!(task_of(&a).id, task_of(&b).id);
    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

// ── Malformed and absent keys ────────────────────────────────────────────

#[tokio::test]
async fn a_send_without_a_key_is_unchanged() {
    // The regression that matters most: nothing about an ordinary send moves.
    let (handler, executions) = counting_handler();
    let a = handler
        .on_send_message(keyed_params("msg-1", None), false, None)
        .await
        .unwrap();
    let b = handler
        .on_send_message(keyed_params("msg-1", None), false, None)
        .await
        .unwrap();

    assert_ne!(
        task_of(&a).id,
        task_of(&b).id,
        "without a key, two sends are two tasks even with one message id"
    );
    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_malformed_key_is_refused_before_anything_runs() {
    let (handler, executions) = counting_handler();
    let mut params = keyed_params("msg-1", None);
    params.message.metadata = Some(serde_json::json!({ IDEMPOTENCY_METADATA_KEY: "short" }));

    let err = handler
        .on_send_message(params, false, None)
        .await
        .expect_err("a key below the minimum length must be refused");
    assert!(matches!(err, ServerError::InvalidParams(_)), "got {err}");
    assert_eq!(executions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_non_string_key_is_refused_rather_than_ignored() {
    // Treating this as "no key" would execute a send the caller believed was
    // deduplicated.
    let (handler, executions) = counting_handler();
    let mut params = keyed_params("msg-1", None);
    params.message.metadata = Some(serde_json::json!({ IDEMPOTENCY_METADATA_KEY: 42 }));

    let err = handler
        .on_send_message(params, false, None)
        .await
        .expect_err("a non-string key must be refused");
    assert!(matches!(err, ServerError::InvalidParams(_)), "got {err}");
    assert_eq!(executions.load(Ordering::SeqCst), 0);
}

// ── Streaming ────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_streaming_retry_replays_the_task_as_its_first_event() {
    // SPEC §3.1.2: the first event of a streaming response is a Task snapshot.
    // For a replay that is the task the original send created, not a new one.
    let (handler, executions) = counting_handler();

    let first = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .unwrap();
    let first_id = task_of(&first).id.clone();

    let replay = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), true, None)
        .await
        .expect("a streaming retry should succeed");

    let SendMessageResult::Stream(mut reader) = replay else {
        panic!("expected a stream");
    };
    let event = reader
        .read()
        .await
        .expect("the stream must open with an event")
        .expect("that event must not be an error");
    match event.event {
        a2a_protocol_types::events::StreamResponse::Task(task) => {
            assert_eq!(task.id, first_id, "the snapshot must be the original task");
        }
        other => panic!("first event must be a Task, got {other:?}"),
    }
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

// ── The store that cannot honour a key ───────────────────────────────────

#[tokio::test]
async fn a_key_is_refused_when_the_store_cannot_honour_it() {
    // Ignoring the key would hand the caller silent at-least-once delivery,
    // which is exactly what presenting a key is meant to rule out.
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::clone(&executions)))
        .with_task_store(NoIdempotencyStore::default())
        .build()
        .expect("build with a store lacking idempotency should succeed");

    let err = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect_err("a keyed send must be refused by a store that cannot dedupe");
    assert!(
        matches!(err, ServerError::UnsupportedOperation(ref m) if m.contains("cannot honour")),
        "unhelpful error: {err}"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "the refused send must not have run the agent"
    );
}

#[tokio::test]
async fn a_store_without_idempotency_still_serves_unkeyed_sends() {
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::clone(&executions)))
        .with_task_store(NoIdempotencyStore::default())
        .build()
        .unwrap();

    handler
        .on_send_message(keyed_params("msg-1", None), false, None)
        .await
        .expect("an unkeyed send must be unaffected");
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

// ── Card advertisement ───────────────────────────────────────────────────

#[tokio::test]
async fn a_store_that_honours_keys_advertises_the_extension() {
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
        .with_agent_card(card())
        .build()
        .unwrap();
    let ext = advertised(&handler).expect("the extension should be advertised");
    assert_eq!(
        ext.required,
        Some(false),
        "requiring it would lock out clients that never send a key"
    );
}

#[tokio::test]
async fn a_store_that_cannot_honour_keys_advertises_nothing() {
    // The pairing that matters: no advertisement means a client knows not to
    // rely on a key, rather than finding out by having a send run twice.
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
        .with_task_store(NoIdempotencyStore::default())
        .with_agent_card(card())
        .build()
        .unwrap();
    assert!(
        advertised(&handler).is_none(),
        "a store without the index must not advertise the extension"
    );
}

#[tokio::test]
async fn an_operators_own_declaration_is_left_alone() {
    use a2a_protocol_types::extensions::AgentExtension;
    let mut c = card();
    c.capabilities.extensions = Some(vec![AgentExtension {
        uri: a2a_protocol_types::idempotency::IDEMPOTENCY_EXTENSION_URI.to_owned(),
        description: Some("operator wording".to_owned()),
        required: Some(true),
        params: None,
    }]);

    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
        .with_agent_card(c)
        .build()
        .unwrap();

    let exts = handler
        .agent_card
        .as_ref()
        .unwrap()
        .capabilities
        .extensions
        .as_ref()
        .unwrap();
    assert_eq!(
        exts.iter()
            .filter(|e| e.uri == a2a_protocol_types::idempotency::IDEMPOTENCY_EXTENSION_URI)
            .count(),
        1,
        "the entry must not be duplicated"
    );
    let ext = advertised(&handler).unwrap();
    assert_eq!(ext.required, Some(true), "the operator's flag must survive");
    assert_eq!(ext.description.as_deref(), Some("operator wording"));
}

#[tokio::test]
async fn a_server_with_no_card_still_builds_and_serves_keys() {
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::clone(&executions)))
        .build()
        .unwrap();
    assert!(handler.agent_card.is_none());

    handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect("a keyed send must still work without a card");
    handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect("and its retry must still replay");
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

// ── Release on failure ───────────────────────────────────────────────────

#[tokio::test]
async fn a_send_that_fails_after_claiming_leaves_the_key_reusable() {
    // A key held by a send that never created a task is worse than no key:
    // the caller's legitimate retry would replay to a task that never
    // existed. Every failure path after the claim releases it.
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::clone(&executions)))
        .with_task_store(FailFirstSaveStore::default())
        .build()
        .unwrap();

    let err = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect_err("the injected store failure should surface");
    assert!(
        !matches!(err, ServerError::TaskNotFound(_)),
        "the first send must fail on the store error, not on idempotency: {err}"
    );

    // The same key and message again. If the failed send had kept the key,
    // this would report the original task as missing instead of running.
    let second = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect("the retry must be able to reclaim the released key");
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "the retry should have run the agent exactly once"
    );
    let _ = task_of(&second);
}
