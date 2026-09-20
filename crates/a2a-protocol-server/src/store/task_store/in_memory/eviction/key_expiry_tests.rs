// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Idempotency keys age out of the in-memory index.
//!
//! Split from `mod.rs` when it crossed the 500-line limit `CONTRIBUTING.md`
//! sets. A key is removed when the send holding it fails and is otherwise
//! kept deliberately — it has to outlive the task it names — so until
//! `TaskStoreConfig::idempotency_key_ttl` existed nothing removed one at all
//! and the index grew for the life of the process.

use super::super::InMemoryTaskStore;
use crate::store::{TaskStore, TaskStoreConfig};
use a2a_protocol_types::message::MessageId;
use a2a_protocol_types::task::TaskId;
use std::time::Duration;

const KEY: &str = "8f14e45fceea167a5a36dedd4bea2543";

fn store_with(ttl: Option<Duration>) -> InMemoryTaskStore {
    InMemoryTaskStore::with_config(
        TaskStoreConfig::default()
            .with_task_ttl(None)
            .with_idempotency_key_ttl(ttl),
    )
}

/// The defect: nothing ever removed a key, so the index grew by one entry
/// per keyed send for the life of the process.
///
/// A key is released when its send fails and is otherwise kept
/// deliberately — it has to outlive the task it names — so neither
/// `task_ttl` nor `max_capacity`, which bound *tasks*, touched it.
#[tokio::test]
async fn a_key_past_its_ttl_is_expired_by_the_sweep() {
    let store = store_with(Some(Duration::from_millis(1)));
    store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
        .await
        .expect("claim");
    assert_eq!(store.idempotency_key_count().await, 1);

    tokio::time::sleep(Duration::from_millis(5)).await;
    store.run_eviction().await;

    assert_eq!(
        store.idempotency_key_count().await,
        0,
        "a key past its TTL must not survive the sweep"
    );
}

/// Counter-test: a key inside its TTL survives, and still replays.
///
/// Without this, an expiry that simply cleared the index would satisfy
/// the test above while destroying the guarantee the key exists for.
#[tokio::test]
async fn a_key_inside_its_ttl_survives_and_still_replays() {
    let store = store_with(Some(Duration::from_secs(3600)));
    store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
        .await
        .expect("claim");

    store.run_eviction().await;

    assert_eq!(store.idempotency_key_count().await, 1);
    let claim = store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t2"))
        .await
        .expect("second claim");
    assert!(
        matches!(claim, crate::store::IdempotencyClaim::Replay(ref id) if id.0 == "t1"),
        "the surviving key must still replay to the first task, got {claim:?}"
    );
}

/// `None` keeps a key for ever — the behaviour of every release before
/// this one, still reachable for a deployment that wants it.
#[tokio::test]
async fn a_none_ttl_keeps_keys_for_ever() {
    let store = store_with(None);
    store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
        .await
        .expect("claim");

    tokio::time::sleep(Duration::from_millis(5)).await;
    store.run_eviction().await;

    assert_eq!(
        store.idempotency_key_count().await,
        1,
        "an opted-out store must keep its keys"
    );
}

/// A replay does not refresh the claim time.
///
/// Otherwise a client retrying on a loop holds a key open indefinitely,
/// and the TTL bounds nothing for exactly the caller most likely to hit
/// it. The window runs from the send the key was first presented with.
#[tokio::test]
async fn a_replay_does_not_extend_the_window() {
    let store = store_with(Some(Duration::from_millis(30)));
    store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
        .await
        .expect("claim");

    // Retry repeatedly across the whole window.
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let _ = store
            .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
            .await
            .expect("replay");
    }
    store.run_eviction().await;

    assert_eq!(
        store.idempotency_key_count().await,
        0,
        "the window runs from the first claim; replays must not extend it"
    );
}
