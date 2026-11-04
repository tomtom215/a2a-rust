// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What `save_appending_history` leaves in each SQL store: the stored history
//! with the new messages appended and trimmed, and every other field from
//! the new snapshot; a task with no record is inserted (audit
//! NEW-h-designs-6). The SQL stores take the trait's default, a read and a
//! write of the whole document, and the trait's docs say so and why.

use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

use super::{HistoryLimit, TaskStore};

fn message(n: usize) -> Message {
    Message::new(
        MessageId::new(format!("m{n}")),
        MessageRole::User,
        vec![Part::text(format!("text {n}"))],
    )
}

fn ids(task: &Task) -> Vec<String> {
    task.history
        .iter()
        .flatten()
        .map(|m| m.id.0.clone())
        .collect()
}

/// The clauses, against any store. `tag` keeps one run's tasks apart from
/// another's in a shared database.
async fn appends_and_trims(store: &dyn TaskStore, tag: &str) {
    let id = format!("{tag}-append");
    let mut task = Task::new(id.as_str(), "ctx", TaskStatus::new(TaskState::Submitted));
    task.history = Some((1..=3).map(message).collect());
    store.save(&task).await.unwrap();

    // The snapshot's own history is ignored: the tail is `messages`.
    let mut next = Task::new(id.as_str(), "ctx", TaskStatus::new(TaskState::Working));
    next.history = Some(vec![message(99)]);
    let limit = HistoryLimit::new(4, usize::MAX);
    store
        .save_appending_history(&next, &[message(4), message(5)], limit)
        .await
        .unwrap();
    let stored = store.get(&TaskId::new(&*id)).await.unwrap().unwrap();
    assert_eq!(
        ids(&stored),
        ["m2", "m3", "m4", "m5"],
        "appended, oldest trimmed"
    );
    assert_eq!(
        stored.status.state,
        TaskState::Working,
        "the snapshot's status"
    );

    // No record: the turn is inserted, not dropped.
    let fresh = format!("{tag}-fresh");
    let first = Task::new(fresh.as_str(), "ctx", TaskStatus::new(TaskState::Submitted));
    store
        .save_appending_history(
            &first,
            &[message(1), message(2), message(3)],
            HistoryLimit::new(2, usize::MAX),
        )
        .await
        .unwrap();
    let stored = store.get(&TaskId::new(&*fresh)).await.unwrap().unwrap();
    assert_eq!(ids(&stored), ["m2", "m3"]);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_stores_append_and_trim() {
    let plain = crate::store::SqliteTaskStore::new("sqlite::memory:")
        .await
        .unwrap();
    appends_and_trims(&plain, "plain").await;
    let tenants = crate::store::TenantAwareSqliteTaskStore::new("sqlite::memory:")
        .await
        .unwrap();
    appends_and_trims(&tenants, "tenant").await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn postgres_stores_append_and_trim() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let tag = format!("t{}", uuid::Uuid::new_v4().simple());
    let plain = crate::store::PostgresTaskStore::new(&url).await.unwrap();
    appends_and_trims(&plain, &tag).await;
    let tenants = crate::store::TenantAwarePostgresTaskStore::new(&url)
        .await
        .unwrap();
    appends_and_trims(&tenants, &tag).await;
}
