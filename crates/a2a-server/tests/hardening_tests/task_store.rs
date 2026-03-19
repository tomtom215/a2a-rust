// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for `InMemoryTaskStore`: CRUD operations, filtering, pagination,
//! TTL eviction, and capacity eviction.

use super::*;

#[tokio::test]
async fn task_store_save_and_get_roundtrip() {
    let store = InMemoryTaskStore::new();
    let task = make_task("t1", "ctx-1", TaskState::Working);

    store.save(task.clone()).await.expect("save");

    let fetched = store
        .get(&TaskId::new("t1"))
        .await
        .expect("get")
        .expect("should be Some");
    assert_eq!(fetched.id, TaskId::new("t1"));
    assert_eq!(fetched.context_id, ContextId::new("ctx-1"));
    assert_eq!(fetched.status.state, TaskState::Working);
}

#[tokio::test]
async fn task_store_get_returns_none_for_missing_task() {
    let store = InMemoryTaskStore::new();

    let result = store
        .get(&TaskId::new("nonexistent"))
        .await
        .expect("get should not error");
    assert!(result.is_none(), "expected None for missing task");
}

#[tokio::test]
async fn task_store_list_with_context_id_filter() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("t1", "ctx-a", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("t2", "ctx-b", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("t3", "ctx-a", TaskState::Completed))
        .await
        .unwrap();

    let params = ListTasksParams {
        tenant: None,
        context_id: Some("ctx-a".into()),
        status: None,
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = store.list(&params).await.expect("list");
    assert_eq!(result.tasks.len(), 2, "should return 2 tasks for ctx-a");
    assert!(result.tasks.iter().all(|t| t.context_id.0 == "ctx-a"));
}

#[tokio::test]
async fn task_store_list_with_status_filter() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("t1", "ctx", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("t2", "ctx", TaskState::Completed))
        .await
        .unwrap();
    store
        .save(make_task("t3", "ctx", TaskState::Completed))
        .await
        .unwrap();

    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: Some(TaskState::Completed),
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = store.list(&params).await.expect("list");
    assert_eq!(result.tasks.len(), 2, "should return 2 completed tasks");
    assert!(result
        .tasks
        .iter()
        .all(|t| t.status.state == TaskState::Completed));
}

#[tokio::test]
async fn task_store_list_with_page_size_limit() {
    let store = InMemoryTaskStore::new();

    for i in 0..10 {
        store
            .save(make_task(&format!("t{i:02}"), "ctx", TaskState::Working))
            .await
            .unwrap();
    }

    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: Some(3),
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = store.list(&params).await.expect("list");
    assert_eq!(
        result.tasks.len(),
        3,
        "should return at most page_size tasks"
    );
}

#[tokio::test]
async fn task_store_delete_removes_task() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("t1", "ctx", TaskState::Working))
        .await
        .unwrap();

    // Confirm present.
    assert!(store.get(&TaskId::new("t1")).await.unwrap().is_some());

    // Delete.
    store.delete(&TaskId::new("t1")).await.expect("delete");

    // Confirm gone.
    assert!(
        store.get(&TaskId::new("t1")).await.unwrap().is_none(),
        "task should be deleted"
    );
}

#[tokio::test]
async fn task_store_ttl_eviction_removes_expired_terminal_tasks() {
    let config = TaskStoreConfig {
        max_capacity: None,
        task_ttl: Some(Duration::from_millis(50)),
        ..Default::default()
    };
    let store = InMemoryTaskStore::with_config(config);

    // Save a completed task.
    store
        .save(make_task("old", "ctx", TaskState::Completed))
        .await
        .unwrap();

    // Wait for the TTL to expire.
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Save another task.
    store
        .save(make_task("new", "ctx", TaskState::Working))
        .await
        .unwrap();

    // Explicitly trigger eviction (eviction is amortized, so may not run on
    // every save; use run_eviction() to test TTL behavior directly).
    store.run_eviction().await;

    // The old completed task should be evicted.
    let old = store.get(&TaskId::new("old")).await.unwrap();
    assert!(
        old.is_none(),
        "expired terminal task should be evicted after TTL"
    );

    // The new working task should still be present.
    let new = store.get(&TaskId::new("new")).await.unwrap();
    assert!(new.is_some(), "non-terminal task should survive TTL check");
}

#[tokio::test]
async fn task_store_ttl_eviction_spares_non_terminal_tasks() {
    let config = TaskStoreConfig {
        max_capacity: None,
        task_ttl: Some(Duration::from_millis(50)),
        ..Default::default()
    };
    let store = InMemoryTaskStore::with_config(config);

    // Save a working (non-terminal) task.
    store
        .save(make_task("working", "ctx", TaskState::Working))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;

    // Save another task.
    store
        .save(make_task("trigger", "ctx", TaskState::Working))
        .await
        .unwrap();

    // Explicitly trigger eviction.
    store.run_eviction().await;

    // Working task should survive since TTL only applies to terminal tasks.
    let working = store.get(&TaskId::new("working")).await.unwrap();
    assert!(
        working.is_some(),
        "non-terminal task should not be evicted by TTL"
    );
}

#[tokio::test]
async fn task_store_capacity_eviction_removes_oldest_terminal_tasks() {
    let config = TaskStoreConfig {
        max_capacity: Some(3),
        task_ttl: None,
        ..Default::default()
    };
    let store = InMemoryTaskStore::with_config(config);

    // Fill the store with terminal tasks.
    store
        .save(make_task("t1", "ctx", TaskState::Completed))
        .await
        .unwrap();
    store
        .save(make_task("t2", "ctx", TaskState::Failed))
        .await
        .unwrap();
    store
        .save(make_task("t3", "ctx", TaskState::Completed))
        .await
        .unwrap();

    // Adding a 4th task should trigger capacity eviction of the oldest terminal.
    store
        .save(make_task("t4", "ctx", TaskState::Working))
        .await
        .unwrap();

    // We should have at most 3 tasks now.
    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: Some(50),
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = store.list(&params).await.unwrap();
    assert!(
        result.tasks.len() <= 3,
        "store should evict to stay within max_capacity, got {} tasks",
        result.tasks.len()
    );

    // The new working task must still be present.
    assert!(store.get(&TaskId::new("t4")).await.unwrap().is_some());
}
