// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Retention against real `SQLite`.
//!
//! Age is simulated by writing `updated_at` directly rather than by sleeping:
//! the policy is measured in days, and a test that waits for one is a test
//! nobody runs.

use super::*;
use crate::store::retention::RetentionPolicy;
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

/// Backdates a row so a policy measured in days can be tested in milliseconds.
async fn age(store: &SqliteTaskStore, id: &str, seconds: i64) {
    sqlx::query(
        "UPDATE tasks SET updated_at = strftime('%Y-%m-%d %H:%M:%f','now', ?1) WHERE id = ?2",
    )
    .bind(format!("-{seconds} seconds"))
    .bind(id)
    .execute(&store.pool)
    .await
    .expect("backdate");
}

async fn ids(store: &SqliteTaskStore) -> Vec<String> {
    sqlx::query_scalar::<_, String>("SELECT id FROM tasks ORDER BY id")
        .fetch_all(&store.pool)
        .await
        .expect("ids")
}

#[tokio::test]
async fn purges_terminal_tasks_past_the_age_and_nothing_else() {
    let store = make_store().await;
    store
        .save(&task("old-done", TaskState::Completed))
        .await
        .unwrap();
    store
        .save(&task("new-done", TaskState::Completed))
        .await
        .unwrap();
    store
        .save(&task("old-working", TaskState::Working))
        .await
        .unwrap();
    store
        .save(&task("old-input", TaskState::InputRequired))
        .await
        .unwrap();
    for id in ["old-done", "old-working", "old-input"] {
        age(&store, id, 7_200).await;
    }

    let report = store
        .purge_expired(&RetentionPolicy::new(Duration::from_secs(3_600)))
        .await
        .expect("purge");

    assert_eq!(report.tasks_deleted, 1, "only the aged terminal task");
    assert!(report.complete);
    assert_eq!(
        ids(&store).await,
        vec!["new-done", "old-input", "old-working"],
        "a Working or InputRequired task is a workflow waiting on someone, \
         not a leak, and must survive any age"
    );
}

#[tokio::test]
async fn keeps_everything_when_nothing_is_old_enough() {
    let store = make_store().await;
    store.save(&task("t1", TaskState::Completed)).await.unwrap();
    let report = store
        .purge_expired(&RetentionPolicy::new(Duration::from_secs(3_600)))
        .await
        .expect("purge");
    assert_eq!(report.tasks_deleted, 0);
    assert_eq!(
        report.batches, 0,
        "no batch should run when nothing matches"
    );
    assert!(report.complete);
    assert_eq!(ids(&store).await, vec!["t1"]);
}

#[tokio::test]
async fn orphan_journal_rows_go_with_their_task() {
    // On the ordinary path a terminal task has no journal rows: `save`
    // supersedes them, and the status transition that makes a task terminal is
    // a full document write. The journal sweep is therefore defensive, and the
    // case it defends against has to be built deliberately — a row that
    // outlived its task because the cascade did not fire, which `journal.rs`
    // records as possible whenever `from_pool` is handed a pool without
    // `foreign_keys=ON`.
    //
    // It matters because `task_artifact_appends` is keyed on the task id: a
    // surviving row would be spliced onto the next task to reuse that id.
    let store = make_store().await;
    store.save(&task("j1", TaskState::Completed)).await.unwrap();
    sqlx::query(
        "INSERT INTO task_artifact_appends (task_id, artifact, seq, part) \
         VALUES ('j1', 0, 0, '{\"kind\":\"text\",\"text\":\"stranded\"}')",
    )
    .execute(&store.pool)
    .await
    .expect("seed journal row");
    age(&store, "j1", 7_200).await;

    let report = store
        .purge_expired(&RetentionPolicy::new(Duration::from_secs(3_600)))
        .await
        .expect("purge");

    assert_eq!(report.tasks_deleted, 1);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM task_artifact_appends")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(left, 0, "the row must not survive its task either way");
    assert_eq!(
        report.journal_orphans_deleted, 0,
        "this pool has foreign_keys=ON, so the cascade took the row and the \
         sweep had nothing of its own to reclaim -- a non-zero count here \
         would mean the cascade had silently not fired"
    );
}

#[tokio::test]
async fn journal_orphans_are_reclaimed_when_the_cascade_does_not_fire() {
    // The case the explicit cleanup exists for, and the only one where the
    // counter is non-zero: a caller's pool without `foreign_keys=ON`, which
    // `from_pool` cannot refuse and cannot detect.
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .pragma("foreign_keys", "OFF")
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("pool");
    let store = SqliteTaskStore::from_pool(pool).await.expect("store");

    store.save(&task("j1", TaskState::Completed)).await.unwrap();
    sqlx::query(
        "INSERT INTO task_artifact_appends (task_id, artifact, seq, part) \
         VALUES ('j1', 0, 0, '{\"kind\":\"text\",\"text\":\"stranded\"}')",
    )
    .execute(&store.pool)
    .await
    .expect("seed");
    age(&store, "j1", 7_200).await;

    let report = store
        .purge_expired(&RetentionPolicy::new(Duration::from_secs(3_600)))
        .await
        .expect("purge");

    assert_eq!(report.tasks_deleted, 1);
    assert_eq!(
        report.journal_orphans_deleted, 1,
        "without the cascade the sweep must reclaim the row itself, and say so"
    );
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM task_artifact_appends")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn a_live_task_keeps_its_journal_rows() {
    // The anti-join deletes rows whose task is gone. A task that survives the
    // sweep must keep its journal, or the next read would splice a truncated
    // artifact.
    let store = make_store().await;
    store.save(&task("keep", TaskState::Working)).await.unwrap();
    sqlx::query(
        "INSERT INTO task_artifact_appends (task_id, artifact, seq, part) \
         VALUES ('keep', 0, 0, '{\"kind\":\"text\",\"text\":\"live\"}')",
    )
    .execute(&store.pool)
    .await
    .expect("seed");
    store
        .save(&task("gone", TaskState::Completed))
        .await
        .unwrap();
    age(&store, "gone", 7_200).await;
    age(&store, "keep", 7_200).await;

    let report = store
        .purge_expired(&RetentionPolicy::new(Duration::from_secs(3_600)))
        .await
        .expect("purge");

    assert_eq!(report.tasks_deleted, 1, "only the terminal one");
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_artifact_appends WHERE task_id = 'keep'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(left, 1, "a surviving task must keep its journal");
}

#[tokio::test]
async fn max_batches_bounds_the_sweep_and_says_it_did() {
    let store = make_store().await;
    for i in 0..5 {
        let id = format!("t{i}");
        store.save(&task(&id, TaskState::Completed)).await.unwrap();
        age(&store, &id, 7_200).await;
    }

    let policy = RetentionPolicy::new(Duration::from_secs(3_600))
        .with_batch_size(2)
        .with_max_batches(2);
    let report = store.purge_expired(&policy).await.expect("purge");

    assert_eq!(report.batches, 2);
    assert_eq!(report.tasks_deleted, 4);
    assert!(
        !report.complete,
        "a caller must be able to tell `nothing left` from `ran out of budget` \
         without inferring it from the count"
    );
    assert_eq!(ids(&store).await.len(), 1);

    // The next sweep finishes the job.
    let rest = store.purge_expired(&policy).await.expect("purge");
    assert_eq!(rest.tasks_deleted, 1);
    assert!(rest.complete);
    assert_eq!(ids(&store).await, [] as [std::string::String; 0]);
}

#[tokio::test]
async fn a_zero_batch_size_still_makes_progress() {
    let store = make_store().await;
    store.save(&task("z", TaskState::Completed)).await.unwrap();
    age(&store, "z", 7_200).await;
    let policy = RetentionPolicy::new(Duration::from_secs(3_600)).with_batch_size(0);
    let report = store.purge_expired(&policy).await.expect("purge");
    assert_eq!(
        report.tasks_deleted, 1,
        "batch size 0 must not mean LIMIT 0"
    );
}
