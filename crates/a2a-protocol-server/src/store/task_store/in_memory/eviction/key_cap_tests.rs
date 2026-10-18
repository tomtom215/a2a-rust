// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The idempotency index holds at most
//! `TaskStoreConfig::max_idempotency_keys` keys (release audit F8).
//!
//! Its TTL bounds how long a key is kept, not how many there are: a client
//! sending keyed messages as fast as it can added one entry per send for a
//! day before the first expired. A full index refuses a *new* key, after
//! expiring what it can, and never evicts a live one to make room.

use super::super::InMemoryTaskStore;
use crate::store::{IdempotencyClaim, TaskStore, TaskStoreConfig};
use a2a_protocol_types::message::MessageId;
use a2a_protocol_types::task::TaskId;
use std::time::Duration;

fn store(max: Option<usize>, ttl: Option<Duration>) -> InMemoryTaskStore {
    InMemoryTaskStore::with_config(
        TaskStoreConfig::default()
            .with_task_ttl(None)
            .with_idempotency_key_ttl(ttl)
            .with_max_idempotency_keys(max),
    )
}

fn key(i: usize) -> String {
    format!("idempotency-key-{i:08}")
}

async fn claim(store: &InMemoryTaskStore, i: usize) -> Result<IdempotencyClaim, String> {
    store
        .claim_idempotency_key(
            &key(i),
            &MessageId::new(format!("m{i}")),
            &TaskId::new(format!("t{i}")),
        )
        .await
        .map_err(|e| e.to_string())
}

/// At the cap a new key is refused as temporarily unavailable, and the
/// keys held are all still there: nothing was evicted to make room.
#[tokio::test]
async fn a_full_index_refuses_a_new_key_and_keeps_every_held_one() {
    let store = store(Some(3), None);
    for i in 0..3 {
        assert_eq!(claim(&store, i).await, Ok(IdempotencyClaim::Claimed));
    }
    let refused = store
        .claim_idempotency_key(&key(3), &MessageId::new("m3"), &TaskId::new("t3"))
        .await
        .expect_err("the index is full");
    assert!(refused.is_unavailable(), "{refused:?}");
    assert!(
        refused.message.contains("idempotency index full"),
        "{refused:?}"
    );
    assert_eq!(store.idempotency_key_count().await, 3);
    for i in 0..3 {
        assert_eq!(
            claim(&store, i).await,
            Ok(IdempotencyClaim::Replay(TaskId::new(format!("t{i}")))),
            "key {i} must still replay"
        );
    }
}

/// A key the store already holds is answered at the cap as below it: a
/// retry of a send that ran is a replay, and a second send under a held key
/// is a conflict — never the full-index refusal.
#[tokio::test]
async fn a_held_key_is_answered_at_the_cap() {
    let store = store(Some(1), None);
    assert_eq!(claim(&store, 0).await, Ok(IdempotencyClaim::Claimed));
    assert_eq!(
        claim(&store, 0).await,
        Ok(IdempotencyClaim::Replay(TaskId::new("t0")))
    );
    let other = store
        .claim_idempotency_key(&key(0), &MessageId::new("another"), &TaskId::new("t9"))
        .await
        .expect("a conflict is an answer, not an error");
    assert_eq!(
        other,
        IdempotencyClaim::Conflict {
            held_by: MessageId::new("m0")
        }
    );
}

/// Below the cap a new key is claimed: the bound is `max`, not `max - 1`.
#[tokio::test]
async fn the_cap_admits_exactly_max_keys() {
    let store = store(Some(2), None);
    assert_eq!(claim(&store, 0).await, Ok(IdempotencyClaim::Claimed));
    assert_eq!(claim(&store, 1).await, Ok(IdempotencyClaim::Claimed));
    assert!(claim(&store, 2).await.is_err());
}

/// A full index expires keys past their TTL before refusing, so a new key
/// is admitted once old ones have aged out, without waiting for a sweep.
#[tokio::test]
async fn a_full_index_expires_old_keys_before_refusing() {
    let store = store(Some(2), Some(Duration::from_millis(20)));
    assert_eq!(claim(&store, 0).await, Ok(IdempotencyClaim::Claimed));
    assert_eq!(claim(&store, 1).await, Ok(IdempotencyClaim::Claimed));
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(claim(&store, 2).await, Ok(IdempotencyClaim::Claimed));
    assert_eq!(store.idempotency_key_count().await, 1);
}

/// With the TTL still running, the full index refuses: expiry runs, but it
/// removes nothing younger than the TTL.
#[tokio::test]
async fn a_full_index_with_young_keys_still_refuses() {
    let store = store(Some(1), Some(Duration::from_secs(3600)));
    assert_eq!(claim(&store, 0).await, Ok(IdempotencyClaim::Claimed));
    assert!(claim(&store, 1).await.is_err());
    assert_eq!(store.idempotency_key_count().await, 1);
}

/// `None` is no limit.
#[tokio::test]
async fn no_cap_admits_any_number() {
    let store = store(None, None);
    for i in 0..50 {
        assert_eq!(claim(&store, i).await, Ok(IdempotencyClaim::Claimed));
    }
}
