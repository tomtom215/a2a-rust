// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The parked sweep on both `SQLite` stores, and the exact bound of its
//! statement under a clock the test sets.

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

use super::*;
use crate::store::{SqliteTaskStore, TaskStore, TenantAwareSqliteTaskStore, TenantContext};

async fn pool() -> SqlitePool {
    // One connection: each connection to `sqlite::memory:` is its own database.
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("pool")
}

fn task(id: &str, state: TaskState) -> Task {
    Task::new(TaskId::new(id), ContextId::new("c"), TaskStatus::new(state))
}

fn event(id: &str) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new(id),
        ContextId::new("c"),
        TaskStatus::new(TaskState::Working),
    ))
}

struct Plain {
    pool: SqlitePool,
    store: SqliteTaskStore,
}

struct Tenant {
    pool: SqlitePool,
    store: TenantAwareSqliteTaskStore,
}

async fn plain() -> Plain {
    let pool = pool().await;
    let store = SqliteTaskStore::from_pool(pool.clone())
        .await
        .expect("store");
    Plain { pool, store }
}

async fn tenant() -> Tenant {
    let pool = pool().await;
    let store = TenantAwareSqliteTaskStore::from_pool(pool.clone())
        .await
        .expect("store");
    Tenant { pool, store }
}

async fn age(pool: &SqlitePool, table: &str, id: &str, seconds: i64) {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {table} SET updated_at = strftime('%Y-%m-%d %H:%M:%f','now', ?1) WHERE id = ?2"
    )))
    .bind(format!("-{seconds} seconds"))
    .bind(id)
    .execute(pool)
    .await
    .expect("backdate");
}

async fn ids(pool: &SqlitePool, table: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM {table} ORDER BY id"
    )))
    .fetch_all(pool)
    .await
    .expect("ids")
}

async fn claim(pool: &SqlitePool, scope: &str, id: &str, live: bool) {
    let expires = crate::store::claim::sqlite::now_ms() + if live { 60_000 } else { -3_600_000 };
    sqlx::query(
        "INSERT INTO a2a_task_claims (scope, task_id, owner, token, cancel_requested, expires_at_ms) \
         VALUES (?1, ?2, 'o', ?3, 0, ?4)",
    )
    .bind(scope)
    .bind(id)
    .bind(format!("tok-{id}"))
    .bind(expires)
    .execute(pool)
    .await
    .expect("claim");
}

async fn count(pool: &SqlitePool, sql: &str, id: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

impl Harness for Plain {
    async fn save(&self, id: &str, state: TaskState) {
        self.store.save(&task(id, state)).await.expect("save");
    }
    async fn age(&self, id: &str, seconds: i64) {
        age(&self.pool, "tasks", id, seconds).await;
    }
    async fn ids(&self) -> Vec<String> {
        ids(&self.pool, "tasks").await
    }
    async fn purge(&self, policy: &RetentionPolicy) -> PurgeReport {
        self.store.purge_expired(policy).await.expect("purge")
    }
    async fn claim(&self, id: &str, live: bool) {
        claim(&self.pool, "", id, live).await;
    }
    async fn append(&self, id: &str, seq: u64) {
        let task = TaskId::new(id);
        self.store
            .append_event(&task, seq, &event(id))
            .await
            .expect("append");
    }
    async fn events(&self, id: &str) -> i64 {
        count(
            &self.pool,
            "SELECT count(*) FROM task_events WHERE task_id = ?1",
            id,
        )
        .await
    }
}

impl Harness for Tenant {
    async fn save(&self, id: &str, state: TaskState) {
        TenantContext::scope("acme", self.store.save(&task(id, state)))
            .await
            .expect("save");
    }
    async fn age(&self, id: &str, seconds: i64) {
        age(&self.pool, "tenant_tasks", id, seconds).await;
    }
    async fn ids(&self) -> Vec<String> {
        ids(&self.pool, "tenant_tasks").await
    }
    async fn purge(&self, policy: &RetentionPolicy) -> PurgeReport {
        self.store.purge_expired(policy).await.expect("purge")
    }
    async fn claim(&self, id: &str, live: bool) {
        claim(&self.pool, "acme", id, live).await;
    }
    async fn append(&self, id: &str, seq: u64) {
        let task = TaskId::new(id);
        TenantContext::scope("acme", self.store.append_event(&task, seq, &event(id)))
            .await
            .expect("append");
    }
    async fn events(&self, id: &str) -> i64 {
        count(
            &self.pool,
            "SELECT count(*) FROM tenant_task_events WHERE task_id = ?1",
            id,
        )
        .await
    }
}

macro_rules! on_both_stores {
    ($($scenario:ident),+ $(,)?) => {$(
        mod $scenario {
            use super::*;
            #[tokio::test]
            async fn plain_sqlite() {
                crate::store::retention::tests::parked::$scenario(&plain().await).await;
            }
            #[tokio::test]
            async fn tenant_sqlite() {
                crate::store::retention::tests::parked::$scenario(&tenant().await).await;
            }
        }
    )+};
}

on_both_stores!(
    default_keeps_every_parked_task,
    opt_in_deletes_only_old_parked_tasks,
    the_two_ages_are_independent,
    a_second_either_side_of_the_age,
    a_live_claim_keeps_a_parked_task,
    batches_are_bounded_and_reported,
    the_log_goes_with_the_task,
);

/// A claim protects the task it names in its own tenant only: the same task id
/// parked under another tenant has no claim and goes.
#[tokio::test]
async fn a_claim_is_scoped_to_its_tenant() {
    let t = tenant().await;
    for tenant in ["acme", "globex"] {
        TenantContext::scope(
            tenant,
            t.store.save(&task("shared", TaskState::InputRequired)),
        )
        .await
        .unwrap();
    }
    sqlx::query(
        "UPDATE tenant_tasks SET updated_at = strftime('%Y-%m-%d %H:%M:%f','now','-7200 seconds')",
    )
    .execute(&t.pool)
    .await
    .unwrap();
    claim(&t.pool, "acme", "shared", true).await;
    let report = t
        .store
        .purge_expired(&policy().with_parked_max_age(Duration::from_secs(3_600)))
        .await
        .unwrap();
    assert_eq!(report.parked_tasks_deleted, 1);
    let left: Vec<String> = sqlx::query_scalar("SELECT tenant_id FROM tenant_tasks")
        .fetch_all(&t.pool)
        .await
        .unwrap();
    assert_eq!(left, ["acme"], "globex's copy had no claim");
}

/// The statement's bound, exactly: a task last updated at `T` is kept at `T +
/// age` (it is not *older* than the age) and goes a millisecond later. The
/// instant ages are measured from is bound, so the test can set it; the real
/// clock never lands on it.
#[tokio::test]
async fn the_bound_is_exact_under_a_clock_the_test_sets() {
    let p = plain().await;
    let sql = crate::store::retention::sqlite::parked_sql("tasks", "''");
    for (now, deleted) in [
        ("2026-01-01 01:00:00.000", false),
        ("2026-01-01 01:00:00.001", true),
        ("2025-12-31 23:59:59.999", false),
    ] {
        sqlx::query("DELETE FROM tasks")
            .execute(&p.pool)
            .await
            .unwrap();
        p.store
            .save(&task("t", TaskState::InputRequired))
            .await
            .unwrap();
        sqlx::query("UPDATE tasks SET updated_at = '2026-01-01 00:00:00.000'")
            .execute(&p.pool)
            .await
            .unwrap();
        let done = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .bind(parked_state_labels()[0].clone())
            .bind(parked_state_labels()[1].clone())
            .bind("-3600 seconds")
            .bind(0_i64)
            .bind(10_i64)
            .bind(now)
            .execute(&p.pool)
            .await
            .unwrap();
        assert_eq!(done.rows_affected() == 1, deleted, "at {now}");
    }
}
