// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `abandoned` leaves out a turn handed back to its caller while its window
//! runs, and finds it once the window has run out (ADR 0054), in each SQL
//! claim store.

#![cfg(any(feature = "sqlite", feature = "postgres"))]

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::task::{Task, TaskState, TaskStatus};

use crate::executor::HAND_OFF_METADATA_KEY;
use crate::store::TaskStore;

/// A working task whose status was written `secs_ago`, marked as handed off
/// when `marked`.
fn working(id: &str, secs_ago: i64, marked: bool) -> Task {
    let at = crate::store::lease::now_unix_ms() - secs_ago * 1_000;
    let mut status = TaskStatus::new(TaskState::Working);
    status.timestamp = Some(a2a_protocol_types::unix_millis_to_iso8601(at));
    let mut note = Message::new(
        MessageId::new(format!("{id}-note")),
        MessageRole::Agent,
        vec![Part::text("handed back")],
    );
    note.metadata = Some(serde_json::json!({ HAND_OFF_METADATA_KEY: marked }));
    status.message = Some(note);
    Task::new(id, "ctx", status)
}

async fn the_window_holds_a_handed_off_task_back(store: Arc<dyn TaskStore>) {
    let claims = store.turn_claims().expect("SQL stores claim turns");
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let id = |n: &str| format!("{tag}-{n}");
    // In its window (60 s of 600), out of it (700 s), and not marked.
    store
        .save(&working(&id("waiting"), 60, true))
        .await
        .unwrap();
    store
        .save(&working(&id("expired"), 700, true))
        .await
        .unwrap();
    store.save(&working(&id("plain"), 60, false)).await.unwrap();
    let mut found: Vec<String> = claims
        .abandoned(Duration::from_secs(3), 1_000)
        .await
        .unwrap()
        .into_iter()
        .map(|(_, task)| task)
        .filter(|task| task.starts_with(&tag))
        .collect();
    found.sort();
    assert_eq!(found, [id("expired"), id("plain")]);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_leaves_a_handed_off_task_to_its_caller() {
    let path = std::env::temp_dir().join(format!("a2a-hand-off-{}.db", uuid::Uuid::new_v4()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let store = Arc::new(crate::store::SqliteTaskStore::new(&url).await.unwrap());
    the_window_holds_a_handed_off_task_back(store).await;
    let tenants = Arc::new(
        crate::store::TenantAwareSqliteTaskStore::new(&url)
            .await
            .unwrap(),
    );
    the_window_holds_a_handed_off_task_back(tenants).await;
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(std::path::PathBuf::from(name));
    }
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn postgres_leaves_a_handed_off_task_to_its_caller() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let store = Arc::new(crate::store::PostgresTaskStore::new(&url).await.unwrap());
    the_window_holds_a_handed_off_task_back(store).await;
    let tenants = Arc::new(
        crate::store::TenantAwarePostgresTaskStore::new(&url)
            .await
            .unwrap(),
    );
    the_window_holds_a_handed_off_task_back(tenants).await;
}
