// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for `TenantAwareSqlitePushConfigStore`.

#![cfg(feature = "sqlite")]

use a2a_protocol_server::push::PushConfigStore;
use a2a_protocol_server::push::TenantAwareSqlitePushConfigStore;
use a2a_protocol_server::store::tenant::TenantContext;
use a2a_protocol_types::push::TaskPushNotificationConfig;

fn make_config(task_id: &str, id: Option<&str>, url: &str) -> TaskPushNotificationConfig {
    TaskPushNotificationConfig {
        tenant: None,
        task_id: task_id.to_string(),
        id: id.map(String::from),
        url: url.to_string(),
        token: None,
        authentication: None,
    }
}

async fn new_store() -> TenantAwareSqlitePushConfigStore {
    TenantAwareSqlitePushConfigStore::new("sqlite::memory:")
        .await
        .expect("failed to create in-memory store")
}

// ── Construction ────────────────────────────────────────────────────────────

#[tokio::test]
async fn new_creates_store() {
    let store = new_store().await;
    // Smoke-test: listing on an empty store should succeed.
    let configs = store.list("nonexistent").await.unwrap();
    assert!(configs.is_empty());
}

// ── Set and get roundtrip ───────────────────────────────────────────────────

#[tokio::test]
async fn set_and_get_roundtrip() {
    let store = new_store().await;
    let config = make_config("task-1", Some("cfg-1"), "https://example.com/hook");

    TenantContext::scope("t1", async {
        let saved = store.set(config).await.unwrap();
        assert_eq!(saved.id.as_deref(), Some("cfg-1"));
        assert_eq!(saved.task_id, "task-1");
        assert_eq!(saved.url, "https://example.com/hook");

        let got = store.get("task-1", "cfg-1").await.unwrap();
        assert!(got.is_some());
        let got = got.unwrap();
        assert_eq!(got.task_id, "task-1");
        assert_eq!(got.id.as_deref(), Some("cfg-1"));
        assert_eq!(got.url, "https://example.com/hook");
    })
    .await;
}

// ── Auto-generated ID ───────────────────────────────────────────────────────

#[tokio::test]
async fn set_auto_generates_id() {
    let store = new_store().await;
    let config = make_config("task-1", None, "https://example.com/hook");

    TenantContext::scope("t1", async {
        let saved = store.set(config).await.unwrap();
        assert!(saved.id.is_some(), "id should be auto-generated");
        assert!(!saved.id.as_ref().unwrap().is_empty());

        // The auto-generated config should be retrievable.
        let id = saved.id.as_ref().unwrap();
        let got = store.get("task-1", id).await.unwrap();
        assert!(got.is_some());
    })
    .await;
}

// ── Upsert behaviour ────────────────────────────────────────────────────────

#[tokio::test]
async fn set_upserts_existing() {
    let store = new_store().await;

    TenantContext::scope("t1", async {
        let c1 = make_config("task-1", Some("cfg-1"), "https://example.com/v1");
        store.set(c1).await.unwrap();

        let c2 = make_config("task-1", Some("cfg-1"), "https://example.com/v2");
        let saved = store.set(c2).await.unwrap();
        assert_eq!(saved.url, "https://example.com/v2");

        let got = store.get("task-1", "cfg-1").await.unwrap().unwrap();
        assert_eq!(got.url, "https://example.com/v2");

        // Should still be only one config for this task.
        let all = store.list("task-1").await.unwrap();
        assert_eq!(all.len(), 1);
    })
    .await;
}

// ── Get nonexistent ─────────────────────────────────────────────────────────

#[tokio::test]
async fn get_nonexistent_returns_none() {
    let store = new_store().await;

    TenantContext::scope("t1", async {
        let got = store.get("no-task", "no-id").await.unwrap();
        assert!(got.is_none());
    })
    .await;
}

// ── List returns all for task ───────────────────────────────────────────────

#[tokio::test]
async fn list_returns_all_for_task() {
    let store = new_store().await;

    TenantContext::scope("t1", async {
        store
            .set(make_config("task-1", Some("a"), "https://a.com"))
            .await
            .unwrap();
        store
            .set(make_config("task-1", Some("b"), "https://b.com"))
            .await
            .unwrap();
        store
            .set(make_config("task-1", Some("c"), "https://c.com"))
            .await
            .unwrap();

        let all = store.list("task-1").await.unwrap();
        assert_eq!(all.len(), 3);

        let mut urls: Vec<String> = all.into_iter().map(|c| c.url).collect();
        urls.sort();
        assert_eq!(
            urls,
            vec!["https://a.com", "https://b.com", "https://c.com"]
        );
    })
    .await;
}

// ── List empty ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_empty_returns_empty_vec() {
    let store = new_store().await;

    TenantContext::scope("t1", async {
        let all = store.list("task-1").await.unwrap();
        assert!(all.is_empty());
    })
    .await;
}

// ── List scoped to task_id ──────────────────────────────────────────────────

#[tokio::test]
async fn list_scoped_to_task_id() {
    let store = new_store().await;

    TenantContext::scope("t1", async {
        store
            .set(make_config("task-A", Some("1"), "https://a.com"))
            .await
            .unwrap();
        store
            .set(make_config("task-A", Some("2"), "https://b.com"))
            .await
            .unwrap();
        store
            .set(make_config("task-B", Some("3"), "https://c.com"))
            .await
            .unwrap();

        let list_a = store.list("task-A").await.unwrap();
        assert_eq!(list_a.len(), 2);

        let list_b = store.list("task-B").await.unwrap();
        assert_eq!(list_b.len(), 1);
        assert_eq!(list_b[0].url, "https://c.com");
    })
    .await;
}

// ── Delete ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_removes_config() {
    let store = new_store().await;

    TenantContext::scope("t1", async {
        store
            .set(make_config("task-1", Some("cfg-1"), "https://example.com"))
            .await
            .unwrap();

        // Verify it exists.
        let before = store.get("task-1", "cfg-1").await.unwrap();
        assert!(before.is_some());

        // Delete it.
        store.delete("task-1", "cfg-1").await.unwrap();

        // Verify it is gone.
        let after = store.get("task-1", "cfg-1").await.unwrap();
        assert!(after.is_none());
    })
    .await;
}

#[tokio::test]
async fn delete_nonexistent_succeeds() {
    let store = new_store().await;

    TenantContext::scope("t1", async {
        // Should not error when deleting something that doesn't exist.
        store.delete("no-task", "no-id").await.unwrap();
    })
    .await;
}

// ── Tenant isolation ────────────────────────────────────────────────────────

#[tokio::test]
async fn tenant_isolation_set_and_get() {
    let store = new_store().await;

    // Tenant A sets a config.
    TenantContext::scope("tenant-a", async {
        store
            .set(make_config("task-1", Some("cfg-1"), "https://a.com"))
            .await
            .unwrap();
    })
    .await;

    // Tenant B cannot see it.
    TenantContext::scope("tenant-b", async {
        let got = store.get("task-1", "cfg-1").await.unwrap();
        assert!(got.is_none(), "tenant-b should not see tenant-a's config");
    })
    .await;

    // Tenant A can still see it.
    TenantContext::scope("tenant-a", async {
        let got = store.get("task-1", "cfg-1").await.unwrap();
        assert!(got.is_some(), "tenant-a should still see its own config");
        assert_eq!(got.unwrap().url, "https://a.com");
    })
    .await;
}

#[tokio::test]
async fn tenant_isolation_list() {
    let store = new_store().await;

    // Tenant A adds two configs.
    TenantContext::scope("tenant-a", async {
        store
            .set(make_config("task-1", Some("a1"), "https://a1.com"))
            .await
            .unwrap();
        store
            .set(make_config("task-1", Some("a2"), "https://a2.com"))
            .await
            .unwrap();
    })
    .await;

    // Tenant B adds one config for the same task_id.
    TenantContext::scope("tenant-b", async {
        store
            .set(make_config("task-1", Some("b1"), "https://b1.com"))
            .await
            .unwrap();
    })
    .await;

    // Each tenant only sees their own.
    TenantContext::scope("tenant-a", async {
        let list = store.list("task-1").await.unwrap();
        assert_eq!(list.len(), 2);
        let mut urls: Vec<String> = list.into_iter().map(|c| c.url).collect();
        urls.sort();
        assert_eq!(urls, vec!["https://a1.com", "https://a2.com"]);
    })
    .await;

    TenantContext::scope("tenant-b", async {
        let list = store.list("task-1").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].url, "https://b1.com");
    })
    .await;
}

#[tokio::test]
async fn tenant_isolation_delete() {
    let store = new_store().await;

    // Tenant A sets a config.
    TenantContext::scope("tenant-a", async {
        store
            .set(make_config("task-1", Some("cfg-1"), "https://a.com"))
            .await
            .unwrap();
    })
    .await;

    // Tenant B tries to delete it — should succeed (no error) but not affect tenant A.
    TenantContext::scope("tenant-b", async {
        store.delete("task-1", "cfg-1").await.unwrap();
    })
    .await;

    // Tenant A's config should still exist.
    TenantContext::scope("tenant-a", async {
        let got = store.get("task-1", "cfg-1").await.unwrap();
        assert!(
            got.is_some(),
            "tenant-b's delete should not affect tenant-a's config"
        );
        assert_eq!(got.unwrap().url, "https://a.com");
    })
    .await;
}
