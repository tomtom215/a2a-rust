// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Edge case tests for push config store.

use a2a_types::push::TaskPushNotificationConfig;

use a2a_server::push::{InMemoryPushConfigStore, PushConfigStore};

#[tokio::test]
async fn delete_nonexistent_config_succeeds() {
    let store = InMemoryPushConfigStore::new();
    // Deleting a config that doesn't exist should not error.
    store.delete("task-1", "nonexistent").await.unwrap();
}

#[tokio::test]
async fn get_nonexistent_config_returns_none() {
    let store = InMemoryPushConfigStore::new();
    let result = store.get("task-1", "nonexistent").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn list_empty_task_returns_empty() {
    let store = InMemoryPushConfigStore::new();
    let configs = store.list("task-1").await.unwrap();
    assert!(configs.is_empty());
}

#[tokio::test]
async fn set_assigns_id_when_missing() {
    let store = InMemoryPushConfigStore::new();
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let stored = store.set(config).await.unwrap();
    assert!(stored.id.is_some(), "ID should be auto-assigned");
}

#[tokio::test]
async fn set_preserves_explicit_id() {
    let store = InMemoryPushConfigStore::new();
    let mut config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    config.id = Some("my-custom-id".into());
    let stored = store.set(config).await.unwrap();
    assert_eq!(stored.id.as_deref(), Some("my-custom-id"));
}

#[tokio::test]
async fn multiple_configs_for_same_task() {
    let store = InMemoryPushConfigStore::new();
    let c1 = TaskPushNotificationConfig::new("task-1", "https://a.example.com/hook");
    let c2 = TaskPushNotificationConfig::new("task-1", "https://b.example.com/hook");

    store.set(c1).await.unwrap();
    store.set(c2).await.unwrap();

    let configs = store.list("task-1").await.unwrap();
    assert_eq!(configs.len(), 2);
}

#[tokio::test]
async fn configs_for_different_tasks_dont_interfere() {
    let store = InMemoryPushConfigStore::new();
    let c1 = TaskPushNotificationConfig::new("task-1", "https://a.example.com/hook");
    let c2 = TaskPushNotificationConfig::new("task-2", "https://b.example.com/hook");

    store.set(c1).await.unwrap();
    store.set(c2).await.unwrap();

    let t1_configs = store.list("task-1").await.unwrap();
    assert_eq!(t1_configs.len(), 1);
    let t2_configs = store.list("task-2").await.unwrap();
    assert_eq!(t2_configs.len(), 1);
}

#[tokio::test]
async fn update_existing_config() {
    let store = InMemoryPushConfigStore::new();
    let mut config = TaskPushNotificationConfig::new("task-1", "https://old.example.com/hook");
    config.id = Some("config-1".into());

    store.set(config).await.unwrap();

    // Update with same ID.
    let mut updated = TaskPushNotificationConfig::new("task-1", "https://new.example.com/hook");
    updated.id = Some("config-1".into());
    store.set(updated).await.unwrap();

    let configs = store.list("task-1").await.unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].url, "https://new.example.com/hook");
}

#[tokio::test]
async fn concurrent_set_operations() {
    let store = std::sync::Arc::new(InMemoryPushConfigStore::new());
    let mut handles = vec![];

    for i in 0..10 {
        let store = std::sync::Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let config =
                TaskPushNotificationConfig::new("task-1", format!("https://hook{i}.example.com"));
            store.set(config).await.unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let configs = store.list("task-1").await.unwrap();
    assert_eq!(
        configs.len(),
        10,
        "all 10 concurrent configs should be stored"
    );
}
