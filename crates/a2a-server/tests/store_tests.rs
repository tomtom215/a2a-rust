// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for task store: pagination, eviction, edge cases.

use std::time::Duration;

use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use a2a_protocol_server::store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};

fn make_task(id: &str, ctx: &str, state: TaskState) -> Task {
    Task {
        id: TaskId::new(id),
        context_id: ContextId::new(ctx),
        status: TaskStatus::new(state),
        history: None,
        artifacts: None,
        metadata: None,
    }
}

fn default_list_params() -> ListTasksParams {
    ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    }
}

// ── Pagination tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn list_with_page_size_truncates() {
    let store = InMemoryTaskStore::new();
    for i in 0..10 {
        store
            .save(make_task(
                &format!("task-{i:02}"),
                "ctx",
                TaskState::Working,
            ))
            .await
            .unwrap();
    }

    let params = ListTasksParams {
        page_size: Some(3),
        ..default_list_params()
    };
    let result = store.list(&params).await.unwrap();
    assert_eq!(result.tasks.len(), 3);
    // Should have a next_page_token since there are more results.
    assert!(result.next_page_token.is_some());
}

#[tokio::test]
async fn list_with_page_token_returns_next_page() {
    let store = InMemoryTaskStore::new();
    for i in 0..10 {
        store
            .save(make_task(
                &format!("task-{i:02}"),
                "ctx",
                TaskState::Working,
            ))
            .await
            .unwrap();
    }

    // Get first page.
    let params = ListTasksParams {
        page_size: Some(3),
        ..default_list_params()
    };
    let page1 = store.list(&params).await.unwrap();
    assert_eq!(page1.tasks.len(), 3);
    let token = page1.next_page_token.unwrap();

    // Get second page using the token.
    let params2 = ListTasksParams {
        page_size: Some(3),
        page_token: Some(token),
        ..default_list_params()
    };
    let page2 = store.list(&params2).await.unwrap();
    assert_eq!(page2.tasks.len(), 3);

    // Pages should not overlap.
    let ids1: Vec<_> = page1.tasks.iter().map(|t| &t.id).collect();
    let ids2: Vec<_> = page2.tasks.iter().map(|t| &t.id).collect();
    for id in &ids2 {
        assert!(!ids1.contains(id), "page 2 should not contain page 1 IDs");
    }
}

#[tokio::test]
async fn list_with_invalid_page_token_returns_empty() {
    let store = InMemoryTaskStore::new();
    store
        .save(make_task("task-1", "ctx", TaskState::Working))
        .await
        .unwrap();

    let params = ListTasksParams {
        page_token: Some("nonexistent-token".into()),
        ..default_list_params()
    };
    let result = store.list(&params).await.unwrap();
    assert!(result.tasks.is_empty());
}

#[tokio::test]
async fn list_last_page_has_no_next_token() {
    let store = InMemoryTaskStore::new();
    for i in 0..3 {
        store
            .save(make_task(&format!("task-{i}"), "ctx", TaskState::Working))
            .await
            .unwrap();
    }

    let params = ListTasksParams {
        page_size: Some(10),
        ..default_list_params()
    };
    let result = store.list(&params).await.unwrap();
    assert_eq!(result.tasks.len(), 3);
    assert!(result.next_page_token.is_none());
}

// ── Filter tests ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_filters_by_context_id() {
    let store = InMemoryTaskStore::new();
    store
        .save(make_task("task-1", "ctx-a", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("task-2", "ctx-b", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("task-3", "ctx-a", TaskState::Completed))
        .await
        .unwrap();

    let params = ListTasksParams {
        context_id: Some("ctx-a".into()),
        ..default_list_params()
    };
    let result = store.list(&params).await.unwrap();
    assert_eq!(result.tasks.len(), 2);
    for task in &result.tasks {
        assert_eq!(task.context_id.0, "ctx-a");
    }
}

#[tokio::test]
async fn list_filters_by_status() {
    let store = InMemoryTaskStore::new();
    store
        .save(make_task("task-1", "ctx", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("task-2", "ctx", TaskState::Completed))
        .await
        .unwrap();
    store
        .save(make_task("task-3", "ctx", TaskState::Working))
        .await
        .unwrap();

    let params = ListTasksParams {
        status: Some(TaskState::Working),
        ..default_list_params()
    };
    let result = store.list(&params).await.unwrap();
    assert_eq!(result.tasks.len(), 2);
}

#[tokio::test]
async fn list_filters_by_context_and_status() {
    let store = InMemoryTaskStore::new();
    store
        .save(make_task("task-1", "ctx-a", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("task-2", "ctx-a", TaskState::Completed))
        .await
        .unwrap();
    store
        .save(make_task("task-3", "ctx-b", TaskState::Working))
        .await
        .unwrap();

    let params = ListTasksParams {
        context_id: Some("ctx-a".into()),
        status: Some(TaskState::Working),
        ..default_list_params()
    };
    let result = store.list(&params).await.unwrap();
    assert_eq!(result.tasks.len(), 1);
    assert_eq!(result.tasks[0].id.0, "task-1");
}

// ── Eviction tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn capacity_eviction_removes_oldest_terminal_tasks() {
    let config = TaskStoreConfig {
        max_capacity: Some(3),
        task_ttl: None,
        ..Default::default()
    };
    let store = InMemoryTaskStore::with_config(config);

    // Add 3 terminal tasks.
    store
        .save(make_task("old-1", "ctx", TaskState::Completed))
        .await
        .unwrap();
    store
        .save(make_task("old-2", "ctx", TaskState::Failed))
        .await
        .unwrap();
    store
        .save(make_task("old-3", "ctx", TaskState::Completed))
        .await
        .unwrap();

    // Add a 4th task — should trigger eviction of oldest.
    store
        .save(make_task("new-1", "ctx", TaskState::Working))
        .await
        .unwrap();

    let result = store.list(&default_list_params()).await.unwrap();
    assert!(result.tasks.len() <= 3, "should respect max capacity");
}

#[tokio::test]
async fn delete_nonexistent_task_succeeds() {
    let store = InMemoryTaskStore::new();
    // Deleting a non-existent task should not error.
    store.delete(&TaskId::new("ghost")).await.unwrap();
}

#[tokio::test]
async fn save_updates_existing_task() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("task-1", "ctx", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("task-1", "ctx", TaskState::Completed))
        .await
        .unwrap();

    let task = store.get(&TaskId::new("task-1")).await.unwrap().unwrap();
    assert_eq!(task.status.state, TaskState::Completed);
}

// ── Edge cases ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn very_large_page_size_returns_all_tasks() {
    let store = InMemoryTaskStore::new();
    for i in 0..5 {
        store
            .save(make_task(&format!("task-{i}"), "ctx", TaskState::Working))
            .await
            .unwrap();
    }

    let params = ListTasksParams {
        page_size: Some(u32::MAX),
        ..default_list_params()
    };
    let result = store.list(&params).await.unwrap();
    assert_eq!(result.tasks.len(), 5);
}

#[tokio::test]
async fn list_empty_store_returns_empty() {
    let store = InMemoryTaskStore::new();
    let result = store.list(&default_list_params()).await.unwrap();
    assert!(result.tasks.is_empty());
}

#[tokio::test]
async fn ttl_eviction_removes_terminal_tasks() {
    let config = TaskStoreConfig {
        max_capacity: None,
        task_ttl: Some(Duration::from_millis(1)),
        ..Default::default()
    };
    let store = InMemoryTaskStore::with_config(config);

    store
        .save(make_task("task-old", "ctx", TaskState::Completed))
        .await
        .unwrap();

    // Sleep to let TTL expire.
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Save another task.
    store
        .save(make_task("task-new", "ctx", TaskState::Working))
        .await
        .unwrap();

    // Explicitly trigger eviction (eviction is amortized).
    store.run_eviction().await;

    let old = store.get(&TaskId::new("task-old")).await.unwrap();
    assert!(old.is_none(), "expired terminal task should be evicted");
}

// ── count() tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn count_returns_zero_for_empty_store() {
    let store = InMemoryTaskStore::new();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn count_tracks_inserts_and_deletes() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("task-1", "ctx", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("task-2", "ctx", TaskState::Working))
        .await
        .unwrap();
    assert_eq!(store.count().await.unwrap(), 2);

    store.delete(&TaskId::new("task-1")).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn count_not_affected_by_update() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("task-1", "ctx", TaskState::Working))
        .await
        .unwrap();
    assert_eq!(store.count().await.unwrap(), 1);

    // Update same task — count should stay at 1.
    store
        .save(make_task("task-1", "ctx", TaskState::Completed))
        .await
        .unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

// ── Multi-tenancy isolation tests ───────────────────────────────────────────

#[tokio::test]
async fn multi_tenant_context_isolation() {
    let store = InMemoryTaskStore::new();

    // Tenant A tasks (context "tenant-a").
    store
        .save(make_task("a-task-1", "tenant-a", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("a-task-2", "tenant-a", TaskState::Completed))
        .await
        .unwrap();

    // Tenant B tasks (context "tenant-b").
    store
        .save(make_task("b-task-1", "tenant-b", TaskState::Working))
        .await
        .unwrap();

    // Listing with tenant-a context should only return tenant-a tasks.
    let params_a = ListTasksParams {
        context_id: Some("tenant-a".into()),
        ..default_list_params()
    };
    let result_a = store.list(&params_a).await.unwrap();
    assert_eq!(result_a.tasks.len(), 2);
    assert!(result_a.tasks.iter().all(|t| t.context_id.0 == "tenant-a"));

    // Listing with tenant-b context should only return tenant-b tasks.
    let params_b = ListTasksParams {
        context_id: Some("tenant-b".into()),
        ..default_list_params()
    };
    let result_b = store.list(&params_b).await.unwrap();
    assert_eq!(result_b.tasks.len(), 1);
    assert_eq!(result_b.tasks[0].id.0, "b-task-1");

    // Total count should include all tenants.
    assert_eq!(store.count().await.unwrap(), 3);
}

#[tokio::test]
async fn multi_tenant_delete_does_not_affect_other_tenants() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("a-1", "tenant-a", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("b-1", "tenant-b", TaskState::Working))
        .await
        .unwrap();

    // Delete tenant-a's task.
    store.delete(&TaskId::new("a-1")).await.unwrap();

    // Tenant-b's task should be unaffected.
    let task_b = store.get(&TaskId::new("b-1")).await.unwrap();
    assert!(task_b.is_some());
    assert_eq!(task_b.unwrap().context_id.0, "tenant-b");

    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn insert_if_absent_returns_correct_count() {
    let store = InMemoryTaskStore::new();

    let inserted = store
        .insert_if_absent(make_task("task-1", "ctx", TaskState::Submitted))
        .await
        .unwrap();
    assert!(inserted);
    assert_eq!(store.count().await.unwrap(), 1);

    // Try inserting same ID again.
    let inserted = store
        .insert_if_absent(make_task("task-1", "ctx", TaskState::Working))
        .await
        .unwrap();
    assert!(!inserted);
    assert_eq!(store.count().await.unwrap(), 1);
}
