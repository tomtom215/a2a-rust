// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Event-log tests for the `SQLite` store.
//!
//! Two things are under test, the same two the idempotency suite covers: the
//! log's own semantics, and that *both* ways of building the schema create
//! the table — the failure migration 5 records about the artifact journal,
//! and the one that would be quietest here, because
//! `supports_event_log()` is a property of the type rather than of the
//! schema, so a missing table shows up as an empty history rather than as an
//! error at startup.

use super::SqliteTaskStore;
use crate::store::TaskStore;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

async fn store() -> SqliteTaskStore {
    SqliteTaskStore::new("sqlite::memory:")
        .await
        .expect("in-memory sqlite should open")
}

fn task(id: &str) -> Task {
    Task {
        id: TaskId::new(id),
        context_id: ContextId::new("c-1"),
        status: TaskStatus::new(TaskState::Working),
        history: None,
        artifacts: None,
        metadata: None,
    }
}

fn event(state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("t-1"),
        context_id: ContextId::new("c-1"),
        status: TaskStatus::new(state),
        metadata: None,
    })
}

fn states(events: &[crate::store::RecordedEvent]) -> Vec<TaskState> {
    events
        .iter()
        .map(|r| match &r.event {
            StreamResponse::StatusUpdate(u) => u.status.state,
            _ => panic!("only status events are written here"),
        })
        .collect()
}

#[tokio::test]
async fn the_sqlite_store_reports_that_it_keeps_a_log() {
    assert!(store().await.supports_event_log());
}

#[tokio::test]
async fn events_round_trip_in_order_with_their_positions() {
    let store = store().await;
    let task = task("t-1");
    store.save(&task).await.expect("save");

    for (seq, state) in [
        (1, TaskState::Submitted),
        (2, TaskState::Working),
        (3, TaskState::Completed),
    ] {
        store
            .append_event(&task.id, seq, &event(state))
            .await
            .expect("append");
    }

    let all = store.read_events(&task.id, 0, 100).await.expect("read");
    assert_eq!(all.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(
        states(&all),
        vec![
            TaskState::Submitted,
            TaskState::Working,
            TaskState::Completed
        ],
        "the payload must survive the round trip, not just the position"
    );
    assert_eq!(store.last_event_seq(&task.id).await.expect("last"), 3);
}

/// `seq` is a position, so the same one written twice leaves one row. This is
/// what makes a retried append safe without a read first.
#[tokio::test]
async fn appending_the_same_position_twice_leaves_one_row() {
    let store = store().await;
    let task = task("t-1");
    store.save(&task).await.expect("save");

    store
        .append_event(&task.id, 1, &event(TaskState::Working))
        .await
        .expect("append");
    store
        .append_event(&task.id, 1, &event(TaskState::Completed))
        .await
        .expect("replay must not error");

    let all = store.read_events(&task.id, 0, 10).await.expect("read");
    assert_eq!(all.len(), 1, "one position, one row");
    assert_eq!(
        states(&all),
        vec![TaskState::Working],
        "the first write wins; a replay must not rewrite history"
    );
}

#[tokio::test]
async fn reading_after_an_offset_is_exclusive_and_honours_the_limit() {
    let store = store().await;
    let task = task("t-1");
    store.save(&task).await.expect("save");
    for seq in 1..=5 {
        store
            .append_event(&task.id, seq, &event(TaskState::Working))
            .await
            .expect("append");
    }

    let after_two = store.read_events(&task.id, 2, 100).await.expect("read");
    assert_eq!(after_two.first().map(|r| r.seq), Some(3), "exclusive");
    assert_eq!(after_two.len(), 3);

    let limited = store.read_events(&task.id, 0, 2).await.expect("read");
    assert_eq!(limited.len(), 2);

    assert!(
        store
            .read_events(&task.id, 99, 10)
            .await
            .expect("read")
            .is_empty(),
        "a subscriber past the end gets nothing, not an error"
    );
}

#[tokio::test]
async fn a_task_with_no_events_reports_zero_rather_than_failing() {
    let store = store().await;
    let missing = TaskId::new("never-existed");
    assert_eq!(store.last_event_seq(&missing).await.expect("last"), 0);
    assert!(
        store
            .read_events(&missing, 0, 10)
            .await
            .expect("read")
            .is_empty()
    );
}

/// The log goes with the task. An orphaned log would be replayed onto a task
/// that later reused the id, and `delete` does this explicitly rather than
/// trusting `ON DELETE CASCADE`, which only fires with `foreign_keys=ON`.
#[tokio::test]
async fn deleting_a_task_removes_its_log() {
    let store = store().await;
    let task = task("t-1");
    store.save(&task).await.expect("save");
    store
        .append_event(&task.id, 1, &event(TaskState::Working))
        .await
        .expect("append");

    store.delete(&task.id).await.expect("delete");
    assert_eq!(store.last_event_seq(&task.id).await.expect("last"), 0);

    // The id is reusable, and must come back with a clean history.
    store.save(&task).await.expect("re-save");
    assert!(
        store
            .read_events(&task.id, 0, 10)
            .await
            .expect("read")
            .is_empty()
    );
}

/// A snapshot rewrite must not touch the log. `save` runs on every status
/// change, and a log that did not survive it would have a completeness that
/// depended on how often the snapshot happened to be written.
#[tokio::test]
async fn a_snapshot_rewrite_leaves_the_log_alone() {
    let store = store().await;
    let mut task = task("t-1");
    store.save(&task).await.expect("save");
    store
        .append_event(&task.id, 1, &event(TaskState::Working))
        .await
        .expect("append");

    task.status = TaskStatus::new(TaskState::Completed);
    store.save(&task).await.expect("re-save");

    assert_eq!(
        store
            .read_events(&task.id, 0, 10)
            .await
            .expect("read")
            .len(),
        1
    );
}

/// Both ways of building the schema must create the table. `SqliteTaskStore::new`
/// runs the migration runner; `from_pool` runs its own inline DDL. Shipping
/// the table in one and not the other is a mistake this repository has made
/// twice, and here it would be silent.
#[tokio::test]
async fn from_pool_creates_the_table_too() {
    let pool = crate::sqlite_pool::sqlite_pool("sqlite::memory:")
        .await
        .expect("pool");
    let store = SqliteTaskStore::from_pool(pool).await.expect("from_pool");

    let task = task("t-1");
    store.save(&task).await.expect("save");
    store
        .append_event(&task.id, 1, &event(TaskState::Working))
        .await
        .expect("a store built by from_pool must have the table too");
    assert_eq!(store.last_event_seq(&task.id).await.expect("last"), 1);
}

/// The other half of the schema question: the migration runner must create it
/// too. The journal shipped created in `from_pool` and absent from the
/// migrations, so the constructor documented as *recommended for production*
/// was the one that did not work.
#[tokio::test]
async fn the_migration_runner_creates_the_table_too() {
    let store = SqliteTaskStore::with_migrations("sqlite::memory:")
        .await
        .expect("migrations");
    let task = task("t-1");
    store.save(&task).await.expect("save");
    store
        .append_event(&task.id, 1, &event(TaskState::Working))
        .await
        .expect("the migration runner must create task_events");
    assert_eq!(store.last_event_seq(&task.id).await.expect("last"), 1);
}
