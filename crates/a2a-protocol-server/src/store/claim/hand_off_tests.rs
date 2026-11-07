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

/// A working task whose status was written `secs_ago`, its status message's
/// hand-off marker set to `marker`.
fn working(id: &str, secs_ago: i64, marker: &serde_json::Value) -> Task {
    let at = crate::store::lease::now_unix_ms() - secs_ago * 1_000;
    let mut status = TaskStatus::new(TaskState::Working);
    status.timestamp = Some(a2a_protocol_types::unix_millis_to_iso8601(at));
    let mut note = Message::new(
        MessageId::new(format!("{id}-note")),
        MessageRole::Agent,
        vec![Part::text("handed back")],
    );
    note.metadata = Some(serde_json::json!({ HAND_OFF_METADATA_KEY: marker }));
    status.message = Some(note);
    Task::new(id, "ctx", status)
}

async fn the_window_holds_a_handed_off_task_back(store: Arc<dyn TaskStore>) {
    let claims = store.turn_claims().expect("SQL stores claim turns");
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let id = |n: &str| format!("{tag}-{n}");
    // In its window (60 s of 600), out of it (700 s), and not marked.
    store
        .save(&working(&id("waiting"), 60, &true.into()))
        .await
        .unwrap();
    store
        .save(&working(&id("expired"), 700, &true.into()))
        .await
        .unwrap();
    store
        .save(&working(&id("plain"), 60, &false.into()))
        .await
        .unwrap();
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
    // A window of 30 s: the hand-off of 60 s ago has run out too.
    let mut found: Vec<String> = claims
        .abandoned_except_handed_off(Duration::from_secs(3), Duration::from_secs(30), 1_000)
        .await
        .unwrap()
        .into_iter()
        .map(|(_, task)| task)
        .filter(|task| task.starts_with(&tag))
        .collect();
    found.sort();
    assert_eq!(found, [id("expired"), id("plain"), id("waiting")]);
    the_published_vectors_mark_exactly_what_they_say(&*store).await;
}

/// The published hand-off vectors (`a2a-protocol-types/extensions/hand-off`,
/// ADR 0049), read by this store, the marker's reader: each valid value holds
/// a task in its window back from `abandoned`; each invalid one does not.
async fn the_published_vectors_mark_exactly_what_they_say(store: &dyn TaskStore) {
    let doc: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../a2a-protocol-types/extensions/hand-off/vectors.json"
    )))
    .unwrap();
    assert_eq!(doc["metadataKey"], HAND_OFF_METADATA_KEY);
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let mut held = Vec::new();
    for (list, marks) in [("valid", true), ("invalid", false)] {
        for (i, case) in doc[list].as_array().unwrap().iter().enumerate() {
            let id = format!("{tag}-{list}-{i}");
            store.save(&working(&id, 60, &case["value"])).await.unwrap();
            held.push((id, marks, case["name"].clone()));
        }
    }
    assert_eq!(held.len(), 5, "the cases the file holds");
    let found: Vec<String> = store
        .turn_claims()
        .unwrap()
        .abandoned(Duration::from_secs(3), 1_000)
        .await
        .unwrap()
        .into_iter()
        .map(|(_, task)| task)
        .collect();
    for (id, marks, name) in held {
        assert_eq!(
            !found.contains(&id),
            marks,
            "{name}: marks a hand-off is {marks}"
        );
    }
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
