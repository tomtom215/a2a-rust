#![cfg(feature = "sqlite")]

use a2a_protocol_server::store::tenant::TenantContext;
use a2a_protocol_server::store::TaskStore;
use a2a_protocol_server::store::TenantAwareSqliteTaskStore;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

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

async fn new_store() -> TenantAwareSqliteTaskStore {
    TenantAwareSqliteTaskStore::new("sqlite::memory:")
        .await
        .expect("failed to create in-memory store")
}

// ── 1. Construction ─────────────────────────────────────────────────────────

#[tokio::test]
async fn new_creates_store() {
    let _store = new_store().await;
    // If we reach here without panic the store was created successfully.
}

// ── 2. Save and get roundtrip ───────────────────────────────────────────────

#[tokio::test]
async fn save_and_get_roundtrip() {
    let store = new_store().await;
    let task = make_task("t1", "ctx1", TaskState::Submitted);

    TenantContext::scope("acme", async {
        store.save(task.clone()).await.unwrap();
        let fetched = store.get(&TaskId::new("t1")).await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id.0, "t1");
        assert_eq!(fetched.context_id.0, "ctx1");
        assert_eq!(fetched.status.state, TaskState::Submitted);
    })
    .await;
}

// ── 3. Get nonexistent returns None ─────────────────────────────────────────

#[tokio::test]
async fn get_nonexistent_returns_none() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        let result = store.get(&TaskId::new("does-not-exist")).await.unwrap();
        assert!(result.is_none());
    })
    .await;
}

// ── 4. Save upserts existing ────────────────────────────────────────────────

#[tokio::test]
async fn save_upserts_existing() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        // Save initial task as Submitted.
        store
            .save(make_task("t1", "ctx1", TaskState::Submitted))
            .await
            .unwrap();
        let fetched = store.get(&TaskId::new("t1")).await.unwrap().unwrap();
        assert_eq!(fetched.status.state, TaskState::Submitted);

        // Overwrite with Working state.
        store
            .save(make_task("t1", "ctx1", TaskState::Working))
            .await
            .unwrap();
        let fetched = store.get(&TaskId::new("t1")).await.unwrap().unwrap();
        assert_eq!(fetched.status.state, TaskState::Working);

        // Count should still be 1 (upsert, not duplicate).
        assert_eq!(store.count().await.unwrap(), 1);
    })
    .await;
}

// ── 5. insert_if_absent returns true then false ─────────────────────────────

#[tokio::test]
async fn insert_if_absent_returns_true_then_false() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        let task = make_task("t1", "ctx1", TaskState::Submitted);

        // First insert succeeds.
        let inserted = store.insert_if_absent(task.clone()).await.unwrap();
        assert!(inserted, "first insert should return true");

        // Second insert with same id returns false.
        let inserted_again = store.insert_if_absent(task).await.unwrap();
        assert!(!inserted_again, "duplicate insert should return false");

        // Task should still exist.
        let fetched = store.get(&TaskId::new("t1")).await.unwrap();
        assert!(fetched.is_some());
    })
    .await;
}

// ── 6. Delete removes task ──────────────────────────────────────────────────

#[tokio::test]
async fn delete_removes_task() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        store
            .save(make_task("t1", "ctx1", TaskState::Submitted))
            .await
            .unwrap();

        // Verify the task exists.
        assert!(store.get(&TaskId::new("t1")).await.unwrap().is_some());

        // Delete it.
        store.delete(&TaskId::new("t1")).await.unwrap();

        // Verify the task is gone.
        assert!(store.get(&TaskId::new("t1")).await.unwrap().is_none());
    })
    .await;
}

// ── 7. Count reflects stored tasks ──────────────────────────────────────────

#[tokio::test]
async fn count_reflects_stored_tasks() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        // Save 3 tasks.
        for i in 1..=3 {
            store
                .save(make_task(&format!("t{i}"), "ctx1", TaskState::Submitted))
                .await
                .unwrap();
        }
        assert_eq!(store.count().await.unwrap(), 3);

        // Delete 1, count should be 2.
        store.delete(&TaskId::new("t2")).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);
    })
    .await;
}

// ── 8. List returns all tasks ───────────────────────────────────────────────

#[tokio::test]
async fn list_returns_all_tasks() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        store
            .save(make_task("t1", "ctx1", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task("t2", "ctx2", TaskState::Working))
            .await
            .unwrap();
        store
            .save(make_task("t3", "ctx1", TaskState::Completed))
            .await
            .unwrap();

        let params = ListTasksParams::default();
        let resp = store.list(&params).await.unwrap();
        assert_eq!(resp.tasks.len(), 3);
        assert!(resp.next_page_token.is_none());
    })
    .await;
}

// ── 9. List filters by context_id ───────────────────────────────────────────

#[tokio::test]
async fn list_filters_by_context_id() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        store
            .save(make_task("t1", "ctx-a", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task("t2", "ctx-b", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task("t3", "ctx-a", TaskState::Working))
            .await
            .unwrap();

        let params = ListTasksParams {
            context_id: Some("ctx-a".to_string()),
            ..Default::default()
        };
        let resp = store.list(&params).await.unwrap();
        assert_eq!(resp.tasks.len(), 2);
        for task in &resp.tasks {
            assert_eq!(task.context_id.0, "ctx-a");
        }
    })
    .await;
}

// ── 10. List filters by status ──────────────────────────────────────────────

#[tokio::test]
async fn list_filters_by_status() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        store
            .save(make_task("t1", "ctx1", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task("t2", "ctx1", TaskState::Working))
            .await
            .unwrap();
        store
            .save(make_task("t3", "ctx1", TaskState::Working))
            .await
            .unwrap();

        let params = ListTasksParams {
            status: Some(TaskState::Working),
            ..Default::default()
        };
        let resp = store.list(&params).await.unwrap();
        assert_eq!(resp.tasks.len(), 2);
        for task in &resp.tasks {
            assert_eq!(task.status.state, TaskState::Working);
        }
    })
    .await;
}

// ── 11. List paginates with page_size ───────────────────────────────────────

#[tokio::test]
async fn list_paginates_with_page_size() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        // Insert 5 tasks with alphabetically ordered IDs.
        for i in 1..=5 {
            store
                .save(make_task(&format!("t{i}"), "ctx1", TaskState::Submitted))
                .await
                .unwrap();
        }

        // Request page 1 with page_size = 2.
        let params = ListTasksParams {
            page_size: Some(2),
            ..Default::default()
        };
        let resp = store.list(&params).await.unwrap();
        assert_eq!(resp.tasks.len(), 2);
        assert!(resp.next_page_token.is_some(), "expected a next page token");
    })
    .await;
}

// ── 12. List paginates with page_token ──────────────────────────────────────

#[tokio::test]
async fn list_paginates_with_page_token() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        for i in 1..=5 {
            store
                .save(make_task(&format!("t{i}"), "ctx1", TaskState::Submitted))
                .await
                .unwrap();
        }

        // Get the first page.
        let params = ListTasksParams {
            page_size: Some(2),
            ..Default::default()
        };
        let resp1 = store.list(&params).await.unwrap();
        assert_eq!(resp1.tasks.len(), 2);
        let token = resp1.next_page_token.clone();
        assert!(token.is_some(), "expected a next page token");

        // Get the second page using the token.
        let params = ListTasksParams {
            page_size: Some(2),
            page_token: token,
            ..Default::default()
        };
        let resp2 = store.list(&params).await.unwrap();
        assert_eq!(resp2.tasks.len(), 2);
        assert!(resp2.next_page_token.is_some());

        // Pages must not overlap.
        let page1_ids: Vec<&str> = resp1.tasks.iter().map(|t| t.id.0.as_str()).collect();
        let page2_ids: Vec<&str> = resp2.tasks.iter().map(|t| t.id.0.as_str()).collect();
        for id in &page2_ids {
            assert!(!page1_ids.contains(id), "page overlap detected for {id}");
        }

        // Get the third page (only 1 task remaining).
        let params = ListTasksParams {
            page_size: Some(2),
            page_token: resp2.next_page_token.clone(),
            ..Default::default()
        };
        let resp3 = store.list(&params).await.unwrap();
        assert_eq!(resp3.tasks.len(), 1);
        assert!(
            resp3.next_page_token.is_none(),
            "last page should have no token"
        );
    })
    .await;
}

// ── 13. List page_size zero uses default ────────────────────────────────────

#[tokio::test]
async fn list_page_size_zero_uses_default() {
    let store = new_store().await;

    TenantContext::scope("acme", async {
        // Insert 3 tasks.
        for i in 1..=3 {
            store
                .save(make_task(&format!("t{i}"), "ctx1", TaskState::Submitted))
                .await
                .unwrap();
        }

        // page_size = 0 should fall back to default (50), returning all 3.
        let params = ListTasksParams {
            page_size: Some(0),
            ..Default::default()
        };
        let resp = store.list(&params).await.unwrap();
        assert_eq!(resp.tasks.len(), 3);
        assert!(resp.next_page_token.is_none());
    })
    .await;
}

// ── 14. Tenant isolation: save and get ──────────────────────────────────────

#[tokio::test]
async fn tenant_isolation_save_and_get() {
    let store = new_store().await;
    let task = make_task("t1", "ctx1", TaskState::Submitted);

    // Tenant A saves a task.
    TenantContext::scope("tenant-a", async {
        store.save(task).await.unwrap();
    })
    .await;

    // Tenant A can see it.
    TenantContext::scope("tenant-a", async {
        let result = store.get(&TaskId::new("t1")).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id.0, "t1");
    })
    .await;

    // Tenant B cannot see it.
    TenantContext::scope("tenant-b", async {
        let result = store.get(&TaskId::new("t1")).await.unwrap();
        assert!(result.is_none());
    })
    .await;
}

// ── 15. Tenant isolation: list ──────────────────────────────────────────────

#[tokio::test]
async fn tenant_isolation_list() {
    let store = new_store().await;

    // Tenant A saves 2 tasks.
    TenantContext::scope("tenant-a", async {
        store
            .save(make_task("a1", "ctx1", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task("a2", "ctx1", TaskState::Working))
            .await
            .unwrap();
    })
    .await;

    // Tenant B saves 1 task.
    TenantContext::scope("tenant-b", async {
        store
            .save(make_task("b1", "ctx1", TaskState::Submitted))
            .await
            .unwrap();
    })
    .await;

    // Tenant A only sees its own 2 tasks.
    TenantContext::scope("tenant-a", async {
        let resp = store.list(&ListTasksParams::default()).await.unwrap();
        assert_eq!(resp.tasks.len(), 2);
        let ids: Vec<&str> = resp.tasks.iter().map(|t| t.id.0.as_str()).collect();
        assert!(ids.contains(&"a1"));
        assert!(ids.contains(&"a2"));
    })
    .await;

    // Tenant B only sees its own 1 task.
    TenantContext::scope("tenant-b", async {
        let resp = store.list(&ListTasksParams::default()).await.unwrap();
        assert_eq!(resp.tasks.len(), 1);
        assert_eq!(resp.tasks[0].id.0, "b1");
    })
    .await;
}

// ── 16. Tenant isolation: count ─────────────────────────────────────────────

#[tokio::test]
async fn tenant_isolation_count() {
    let store = new_store().await;

    // Tenant A saves 3 tasks.
    TenantContext::scope("tenant-a", async {
        for i in 1..=3 {
            store
                .save(make_task(&format!("a{i}"), "ctx1", TaskState::Submitted))
                .await
                .unwrap();
        }
        assert_eq!(store.count().await.unwrap(), 3);
    })
    .await;

    // Tenant B saves 1 task.
    TenantContext::scope("tenant-b", async {
        store
            .save(make_task("b1", "ctx1", TaskState::Submitted))
            .await
            .unwrap();
        assert_eq!(store.count().await.unwrap(), 1);
    })
    .await;

    // Tenant A count is still 3.
    TenantContext::scope("tenant-a", async {
        assert_eq!(store.count().await.unwrap(), 3);
    })
    .await;
}

// ── 17. Tenant isolation: delete ────────────────────────────────────────────

#[tokio::test]
async fn tenant_isolation_delete() {
    let store = new_store().await;

    // Tenant A saves a task.
    TenantContext::scope("tenant-a", async {
        store
            .save(make_task("t1", "ctx1", TaskState::Submitted))
            .await
            .unwrap();
    })
    .await;

    // Tenant B tries to delete tenant A's task — should have no effect.
    TenantContext::scope("tenant-b", async {
        store.delete(&TaskId::new("t1")).await.unwrap();
    })
    .await;

    // Tenant A's task should still exist.
    TenantContext::scope("tenant-a", async {
        let result = store.get(&TaskId::new("t1")).await.unwrap();
        assert!(result.is_some(), "tenant-b delete must not affect tenant-a");
    })
    .await;
}

// ── 18. Tenant isolation: insert_if_absent ──────────────────────────────────

#[tokio::test]
async fn tenant_isolation_insert_if_absent() {
    let store = new_store().await;

    // Tenant A inserts task with id "t1".
    let inserted_a = TenantContext::scope("tenant-a", async {
        let task = make_task("t1", "ctx1", TaskState::Submitted);
        store.insert_if_absent(task).await.unwrap()
    })
    .await;
    assert!(inserted_a, "tenant-a first insert should return true");

    // Tenant B inserts task with same id "t1" — should also succeed since
    // the primary key is (tenant_id, id).
    let inserted_b = TenantContext::scope("tenant-b", async {
        let task = make_task("t1", "ctx1", TaskState::Working);
        store.insert_if_absent(task).await.unwrap()
    })
    .await;
    assert!(
        inserted_b,
        "tenant-b insert of same task id should return true (different tenant)"
    );

    // Both tenants can see their own version.
    TenantContext::scope("tenant-a", async {
        let task = store.get(&TaskId::new("t1")).await.unwrap().unwrap();
        assert_eq!(task.status.state, TaskState::Submitted);
    })
    .await;

    TenantContext::scope("tenant-b", async {
        let task = store.get(&TaskId::new("t1")).await.unwrap().unwrap();
        assert_eq!(task.status.state, TaskState::Working);
    })
    .await;
}
