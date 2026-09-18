// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Idempotency-key tests for the `SQLite` store.
//!
//! Two things are under test. The claim's three outcomes and its atomicity
//! under concurrency; and that *both* ways of building the schema create the
//! table, which is the failure migration 5 records about the artifact journal.

use super::SqliteTaskStore;
use crate::store::TaskStore;
use crate::store::task_store::IdempotencyClaim;
use a2a_protocol_types::message::MessageId;
use a2a_protocol_types::task::TaskId;

const KEY: &str = "8f14e45fceea167a5a36dedd4bea2543";

async fn store() -> SqliteTaskStore {
    SqliteTaskStore::new("sqlite::memory:")
        .await
        .expect("in-memory sqlite should open")
}

#[tokio::test]
async fn the_sqlite_store_reports_that_it_honours_keys() {
    assert!(store().await.supports_idempotency());
}

#[tokio::test]
async fn a_free_key_is_claimed() {
    let store = store().await;
    assert_eq!(
        store
            .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
            .await
            .unwrap(),
        IdempotencyClaim::Claimed
    );
}

#[tokio::test]
async fn the_same_message_replays_to_the_first_task() {
    let store = store().await;
    let msg = MessageId::new("m1");
    store
        .claim_idempotency_key(KEY, &msg, &TaskId::new("t1"))
        .await
        .unwrap();
    assert_eq!(
        store
            .claim_idempotency_key(KEY, &msg, &TaskId::new("t2"))
            .await
            .unwrap(),
        IdempotencyClaim::Replay(TaskId::new("t1"))
    );
}

#[tokio::test]
async fn a_different_message_conflicts_and_names_the_holder() {
    let store = store().await;
    store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
        .await
        .unwrap();
    assert_eq!(
        store
            .claim_idempotency_key(KEY, &MessageId::new("m2"), &TaskId::new("t2"))
            .await
            .unwrap(),
        IdempotencyClaim::Conflict {
            held_by: MessageId::new("m1")
        }
    );
}

#[tokio::test]
async fn a_released_key_can_be_claimed_again() {
    let store = store().await;
    store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
        .await
        .unwrap();
    store.release_idempotency_key(KEY).await.unwrap();
    assert_eq!(
        store
            .claim_idempotency_key(KEY, &MessageId::new("m2"), &TaskId::new("t2"))
            .await
            .unwrap(),
        IdempotencyClaim::Claimed,
        "a released key must be free for anyone, including a different message"
    );
}

#[tokio::test]
async fn releasing_a_key_nobody_holds_is_not_an_error() {
    // A failure path may run after another caller has taken over.
    store().await.release_idempotency_key(KEY).await.unwrap();
}

#[tokio::test]
async fn a_key_survives_the_deletion_of_its_task() {
    // Deliberately no foreign key: a cascade would free the key when a
    // retention sweep removed the task, and the next retry would then execute
    // the send a second time.
    use a2a_protocol_types::task::{ContextId, Task, TaskState, TaskStatus};

    let store = store().await;
    let task = Task {
        id: TaskId::new("t1"),
        context_id: ContextId::new("ctx"),
        status: TaskStatus::new(TaskState::Completed),
        history: None,
        artifacts: None,
        metadata: None,
    };
    store.save(&task).await.unwrap();
    store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &task.id)
        .await
        .unwrap();

    store.delete(&task.id).await.unwrap();

    assert_eq!(
        store
            .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t2"))
            .await
            .unwrap(),
        IdempotencyClaim::Replay(TaskId::new("t1")),
        "the key must still name the swept task, not be free to re-run"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_claims_of_one_key_produce_exactly_one_winner() {
    use std::sync::Arc;

    // A shared file-backed database, because `sqlite::memory:` gives each
    // pool connection its own private database — concurrent claims would not
    // meet, and the test would pass without proving anything.
    let dir = std::env::temp_dir().join(format!("a2a-idem-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("claims.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());

    let store = Arc::new(
        SqliteTaskStore::new(&url)
            .await
            .expect("file-backed sqlite"),
    );
    let msg = MessageId::new("m1");

    let mut claims = Vec::new();
    for i in 0..16 {
        let store = Arc::clone(&store);
        let msg = msg.clone();
        claims.push(tokio::spawn(async move {
            store
                .claim_idempotency_key(KEY, &msg, &TaskId::new(format!("t{i}")))
                .await
        }));
    }

    let mut claimed = 0;
    let mut replays = 0;
    for c in claims {
        match c.await.unwrap().expect("claim must not error") {
            IdempotencyClaim::Claimed => claimed += 1,
            IdempotencyClaim::Replay(_) => replays += 1,
            IdempotencyClaim::Conflict { held_by } => {
                panic!("one message cannot conflict with itself (held_by {held_by})")
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(claimed, 1, "exactly one claim may win");
    assert_eq!(replays, 15);
}

#[tokio::test]
async fn both_ways_of_building_the_schema_create_the_table() {
    // The journal shipped created in `from_pool` and absent from the
    // migrations, so the constructor documented as recommended for production
    // was the one that did not work. Each path is exercised here.
    let via_new = SqliteTaskStore::new("sqlite::memory:").await.unwrap();
    assert_eq!(
        via_new
            .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
            .await
            .unwrap(),
        IdempotencyClaim::Claimed,
        "`new` must create idempotency_keys"
    );

    let via_migrations = SqliteTaskStore::with_migrations("sqlite::memory:")
        .await
        .unwrap();
    assert_eq!(
        via_migrations
            .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t1"))
            .await
            .unwrap(),
        IdempotencyClaim::Claimed,
        "the migration runner must create idempotency_keys"
    );
}
