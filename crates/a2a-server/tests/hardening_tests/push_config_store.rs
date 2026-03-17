// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for `InMemoryPushConfigStore`: CRUD lifecycle, missing lookups,
//! auto-assigned IDs, and explicit ID preservation.

use super::*;

#[tokio::test]
async fn push_config_store_crud_lifecycle() {
    let store = InMemoryPushConfigStore::new();

    // Set.
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let saved = store.set(config).await.expect("set");
    assert!(saved.id.is_some(), "should have an assigned ID");
    let config_id = saved.id.clone().unwrap();

    // Get.
    let fetched = store
        .get("task-1", &config_id)
        .await
        .expect("get")
        .expect("should be Some");
    assert_eq!(fetched.url, "https://example.com/hook");
    assert_eq!(fetched.task_id, "task-1");

    // List.
    let configs = store.list("task-1").await.expect("list");
    assert_eq!(configs.len(), 1);

    // Delete.
    store.delete("task-1", &config_id).await.expect("delete");

    // Verify gone.
    let after_delete = store.get("task-1", &config_id).await.expect("get");
    assert!(after_delete.is_none(), "config should be deleted");

    let list_after = store.list("task-1").await.expect("list");
    assert!(list_after.is_empty(), "list should be empty after delete");
}

#[tokio::test]
async fn push_config_store_get_returns_none_for_missing_config() {
    let store = InMemoryPushConfigStore::new();

    let result = store
        .get("task-missing", "id-missing")
        .await
        .expect("get should not error");
    assert!(result.is_none(), "expected None for missing config");
}

#[tokio::test]
async fn push_config_store_auto_assigns_id_if_not_present() {
    let store = InMemoryPushConfigStore::new();

    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    assert!(config.id.is_none(), "new config should not have ID yet");

    let saved = store.set(config).await.expect("set");
    assert!(saved.id.is_some(), "store should auto-assign an ID");
    assert!(
        !saved.id.as_ref().unwrap().is_empty(),
        "assigned ID should be non-empty"
    );
}

#[tokio::test]
async fn push_config_store_preserves_explicit_id() {
    let store = InMemoryPushConfigStore::new();

    let mut config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    config.id = Some("my-custom-id".into());

    let saved = store.set(config).await.expect("set");
    assert_eq!(
        saved.id.as_deref(),
        Some("my-custom-id"),
        "explicit ID should be preserved"
    );

    // Retrieve by the explicit ID.
    let fetched = store
        .get("task-1", "my-custom-id")
        .await
        .expect("get")
        .expect("should find by explicit ID");
    assert_eq!(fetched.url, "https://example.com/hook");
}
