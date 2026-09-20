// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The retention sweep expires idempotency keys.
//!
//! Split from `retention_tests.rs` when it crossed the 500-line limit
//! `CONTRIBUTING.md` sets. Kept apart from the task and orphan sweeps on
//! purpose: those reclaim rows that should not exist, while an expired key is
//! a row whose time is simply up — and deleting one early is a send that can
//! execute twice, which is a different kind of mistake from leaving an orphan
//! behind.

use super::*;
use crate::store::retention::RetentionPolicy;
use a2a_protocol_types::message::MessageId;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
use std::time::Duration;

async fn make_store() -> SqliteTaskStore {
    SqliteTaskStore::new("sqlite::memory:")
        .await
        .expect("store")
}

fn task(id: &str, state: TaskState) -> Task {
    Task {
        id: TaskId::new(id),
        context_id: ContextId::new("ctx"),
        status: TaskStatus::new(state),
        history: None,
        artifacts: None,
        metadata: None,
    }
}

/// Backdates a key so a policy measured in days can be tested in seconds.
async fn age_key(store: &SqliteTaskStore, key: &str, seconds: i64) {
    sqlx::query(
        "UPDATE idempotency_keys SET created_at = strftime('%Y-%m-%d %H:%M:%S','now', ?1) \
         WHERE key = ?2",
    )
    .bind(format!("-{seconds} seconds"))
    .bind(key)
    .execute(&store.pool)
    .await
    .expect("backdate key");
}

async fn key_count(store: &SqliteTaskStore) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM idempotency_keys")
        .fetch_one(&store.pool)
        .await
        .expect("count keys")
}

const OLD_KEY: &str = "8f14e45fceea167a5a36dedd4bea2543";
const NEW_KEY: &str = "0123456789abcdef0123456789abcdef";

/// The defect: nothing deleted an idempotency key, ever.
///
/// A key is removed when the send holding it fails, and otherwise kept
/// deliberately — it has to outlive the task it names. The retention sweep
/// deleted tasks, the journal and the event log, and left the key table to
/// grow for the life of the database.
#[tokio::test]
async fn the_sweep_expires_old_keys_and_keeps_recent_ones() {
    let store = make_store().await;
    store
        .save(&task("t1", TaskState::Completed))
        .await
        .expect("save");
    for key in [OLD_KEY, NEW_KEY] {
        store
            .claim_idempotency_key(key, &MessageId::new("m1"), &TaskId::new("t1"))
            .await
            .expect("claim");
    }
    age_key(&store, OLD_KEY, 7200).await;
    assert_eq!(key_count(&store).await, 2, "precondition: two keys");

    // An hour's key age, and a task age of one second so the clamp does not
    // raise the key age above the two hours the old key was backdated by.
    let report = store
        .purge_expired(
            &RetentionPolicy::new(Duration::from_secs(1))
                .with_idempotency_key_max_age(Some(Duration::from_secs(3600))),
        )
        .await
        .expect("purge");

    assert_eq!(
        report.idempotency_keys_deleted, 1,
        "exactly the expired key, and the report says so"
    );
    let remaining: Vec<String> =
        sqlx::query_scalar::<_, String>("SELECT key FROM idempotency_keys")
            .fetch_all(&store.pool)
            .await
            .expect("remaining");
    assert_eq!(
        remaining,
        vec![NEW_KEY.to_owned()],
        "the recent key must survive: expiring it early is a send that runs twice"
    );
}

/// Counter-test: opting out keeps every key, which is the pre-0.14 behaviour.
#[tokio::test]
async fn a_policy_without_a_key_age_deletes_no_keys() {
    let store = make_store().await;
    store
        .save(&task("t1", TaskState::Completed))
        .await
        .expect("save");
    store
        .claim_idempotency_key(OLD_KEY, &MessageId::new("m1"), &TaskId::new("t1"))
        .await
        .expect("claim");
    age_key(&store, OLD_KEY, 7_000_000).await;

    let report = store
        .purge_expired(
            &RetentionPolicy::new(Duration::from_secs(1)).with_idempotency_key_max_age(None),
        )
        .await
        .expect("purge");

    assert_eq!(report.idempotency_keys_deleted, 0);
    assert_eq!(
        key_count(&store).await,
        1,
        "a store that opted out must keep its keys however old they are"
    );
}

/// The clamp reaches the SQL, not just the accessor.
///
/// A key must outlive the task it names. With a thirty-day task age and a
/// one-minute key age, the sweep has to use thirty days — otherwise it
/// deletes a key whose task is still in the table, and the next retry creates
/// a second task rather than replaying.
#[tokio::test]
async fn the_sweep_will_not_expire_a_key_younger_than_the_task_age() {
    let store = make_store().await;
    store
        .save(&task("t1", TaskState::Completed))
        .await
        .expect("save");
    store
        .claim_idempotency_key(OLD_KEY, &MessageId::new("m1"), &TaskId::new("t1"))
        .await
        .expect("claim");
    // An hour old: past the one-minute key age asked for, far short of the
    // thirty-day task age the clamp raises it to.
    age_key(&store, OLD_KEY, 3600).await;

    let report = store
        .purge_expired(
            &RetentionPolicy::new(Duration::from_secs(30 * 24 * 3600))
                .with_idempotency_key_max_age(Some(Duration::from_secs(60))),
        )
        .await
        .expect("purge");

    assert_eq!(report.idempotency_keys_deleted, 0);
    assert_eq!(
        key_count(&store).await,
        1,
        "the clamp must hold in the sweep: this key's task is still retained"
    );
}
