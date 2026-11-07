// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The parked sweep on both `PostgreSQL` stores, and the exact bound of its
//! statement inside a transaction, where `now()` stands still.

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
use sqlx::postgres::PgPool;

use super::*;
use crate::store::tests::pg_scratch::Scratch;
use crate::store::{PostgresTaskStore, TaskStore, TenantAwarePostgresTaskStore, TenantContext};

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
    db: Scratch,
    pool: PgPool,
    store: PostgresTaskStore,
}

struct Tenant {
    db: Scratch,
    pool: PgPool,
    store: TenantAwarePostgresTaskStore,
}

async fn plain() -> Plain {
    let db = Scratch::new("parked").await;
    let pool = db.pool(4).await;
    let store = PostgresTaskStore::from_pool(pool.clone()).await.unwrap();
    Plain { db, pool, store }
}

async fn tenant() -> Tenant {
    let db = Scratch::new("parked").await;
    let pool = db.pool(4).await;
    let store = TenantAwarePostgresTaskStore::from_pool(pool.clone())
        .await
        .unwrap();
    Tenant { db, pool, store }
}

async fn age(pool: &PgPool, table: &str, id: &str, seconds: i64) {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {table} SET updated_at = now() - ($1 || ' seconds')::interval WHERE id = $2"
    )))
    .bind(seconds.to_string())
    .bind(id)
    .execute(pool)
    .await
    .expect("backdate");
}

async fn ids(pool: &PgPool, table: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM {table} ORDER BY id"
    )))
    .fetch_all(pool)
    .await
    .expect("ids")
}

async fn claim(pool: &PgPool, scope: &str, id: &str, live: bool) {
    let when = if live {
        "now() + interval '1 minute'"
    } else {
        "now() - interval '1 hour'"
    };
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO a2a_task_claims (scope, task_id, owner, token, expires_at) \
         VALUES ($1, $2, 'o', $3, {when})"
    )))
    .bind(scope)
    .bind(id)
    .bind(format!("tok-{id}"))
    .execute(pool)
    .await
    .expect("claim");
}

async fn count(pool: &PgPool, table: &str, id: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM {table} WHERE task_id = $1"
    )))
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
        count(&self.pool, "task_events", id).await
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
        count(&self.pool, "tenant_task_events", id).await
    }
}

macro_rules! on_both_stores {
    ($($scenario:ident),+ $(,)?) => {$(
        mod $scenario {
            use super::*;
            #[tokio::test]
            #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
            async fn plain_postgres() {
                let h = plain().await;
                crate::store::retention::tests::parked::$scenario(&h).await;
                h.pool.close().await;
                h.db.drop_database().await;
            }
            #[tokio::test]
            #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
            async fn tenant_postgres() {
                let h = tenant().await;
                crate::store::retention::tests::parked::$scenario(&h).await;
                h.pool.close().await;
                h.db.drop_database().await;
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

/// A claim protects the task it names in its own tenant only.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
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
    sqlx::query("UPDATE tenant_tasks SET updated_at = now() - interval '2 hours'")
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
    t.pool.close().await;
    t.db.drop_database().await;
}

/// The statement's bound, exactly. The sweep's own text runs in a transaction
/// that also writes the row, so `now()` is the same instant for both and an age
/// can be made equal to the policy's to the microsecond, which the moving clock
/// of a real sweep never allows: a task that is exactly the age is kept (it is
/// not *older*), a microsecond past it goes, a microsecond short stays.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn the_bound_is_exact_with_now_held_still() {
    let p = plain().await;
    let sql = crate::store::retention::postgres::parked_sql("tasks", "''");
    for (extra_age_us, deleted) in [(-1, false), (0, false), (1, true)] {
        let mut tx = p.pool.begin().await.unwrap();
        sqlx::query("DELETE FROM tasks")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tasks (id, context_id, state, data, updated_at) \
             VALUES ('t', 'c', 'TASK_STATE_INPUT_REQUIRED', '{}'::jsonb, \
                     now() - interval '3600 seconds' - make_interval(secs => $1))",
        )
        .bind(f64::from(extra_age_us) / 1e6)
        .execute(&mut *tx)
        .await
        .unwrap();
        let done = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .bind(parked_state_labels())
            .bind("3600 seconds")
            .bind(10_i64)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            done.rows_affected() == 1,
            deleted,
            "{extra_age_us} us past the age"
        );
        tx.rollback().await.unwrap();
    }
    p.pool.close().await;
    p.db.drop_database().await;
}

/// A row another transaction holds, a continuation that has written, is passed
/// over and not waited for (`FOR UPDATE OF t SKIP LOCKED`): the sweep finishes
/// while the row is locked, deleting the one that is not, and the locked one
/// is still there.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn a_row_another_transaction_holds_is_skipped_not_waited_for() {
    let p = plain().await;
    for id in ["held", "free"] {
        p.store
            .save(&task(id, TaskState::InputRequired))
            .await
            .unwrap();
        age(&p.pool, "tasks", id, 2 * HOUR).await;
    }
    let mut tx = p.pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM tasks WHERE id = 'held' FOR UPDATE")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    let policy = policy().with_parked_max_age(Duration::from_hours(1));
    let sweep = p.store.purge_expired(&policy);
    let report = tokio::time::timeout(Duration::from_secs(10), sweep)
        .await
        .expect("the sweep waited for a row another transaction holds")
        .unwrap();
    assert_eq!(report.parked_tasks_deleted, 1);
    tx.rollback().await.unwrap();
    assert_eq!(ids(&p.pool, "tasks").await, ["held"]);
    p.pool.close().await;
    p.db.drop_database().await;
}
