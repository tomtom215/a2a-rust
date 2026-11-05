// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The plan PostgreSQL chooses for the reaper's query on the tenant-aware
//! table (audit NEW-h-s0-8).
//!
//! The audit's conjecture was that, with `tenant_id` leading every index of
//! `tenant_tasks` and no tenant in the query, the planner scans the whole
//! table every claim time to live on every replica. Measured on 2026-11-04
//! (PostgreSQL 16.15, 500,000 rows over 2,000 tenants, 50 `working`): the
//! planner reads `idx_tenant_tasks_state` with `state = ANY(..)` as its
//! index condition, 1,215 buffers and 3.0 ms, against a 163 MB heap and a
//! 4.7 MB index. So the cost grows with the index (about 10 bytes a row),
//! not the table. The decision: no partial index for
//! now; one on `(updated_at) WHERE state IN (submitted, working)` would make
//! the look proportional to the running tasks, at the price of one more
//! index write per write of a running task, and is worth it at tens of
//! millions of retained rows. This test keeps the measured plan from
//! regressing: dropping or reshaping the state index turns it into a
//! sequential scan of the table, which it refuses.

#![cfg(feature = "postgres")]

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// Rows in the test table: enough that the planner prefers an index, far
/// fewer than the measurement's 500,000, so the test runs in about a second.
const ROWS: i64 = 20_000;

#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn the_reapers_tenant_query_reads_the_state_index_not_the_table() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let (admin, pool, schema) = seeded(&url).await;

    let explain = format!(
        "EXPLAIN (FORMAT JSON) {}",
        super::postgres::ABANDONED_TENANT_SQL
    );
    let (plan,): (serde_json::Value,) =
        crate::store::pg_retry::retry(&crate::metrics::NoopMetrics, || {
            sqlx::query_as(sqlx::AssertSqlSafe(explain.as_str()))
                .bind(15_000_i64)
                .bind(32_i64)
                .bind(600_000_i64)
                .fetch_one(&pool)
        })
        .await
        .unwrap();
    // The reaper finds exactly the two running rows through that plan.
    let found: Vec<(String, String)> =
        crate::store::pg_retry::retry(&crate::metrics::NoopMetrics, || {
            sqlx::query_as(super::postgres::ABANDONED_TENANT_SQL)
                .bind(15_000_i64)
                .bind(32_i64)
                .bind(600_000_i64)
                .fetch_all(&pool)
        })
        .await
        .unwrap();
    crate::store::pg_retry::checked(
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE"))).execute(&admin),
    )
    .await
    .unwrap();

    let mut scans = Vec::new();
    scans_of(&plan, "tenant_tasks", &mut scans);
    assert_eq!(
        scans,
        [(
            "Index Scan".to_owned(),
            Some("idx_tenant_tasks_state".to_owned())
        )],
        "{plan:#}"
    );
    let mut found: Vec<_> = found.into_iter().map(|(_, id)| id).collect();
    found.sort();
    assert_eq!(found, ["task-10000", "task-20000"]);
}

/// A schema of its own holding the store's tables, `ROWS` tasks of which
/// every ten-thousandth is running, analysed: the admin pool, a pool whose
/// search path is the schema, and the schema's name.
async fn seeded(url: &str) -> (sqlx::postgres::PgPool, sqlx::postgres::PgPool, String) {
    // A schema of its own, so the rows below meet no other test's reaper and
    // no other test's rows change the statistics.
    let schema = format!("plan_{}", uuid::Uuid::new_v4().simple());
    let admin = sqlx::postgres::PgPool::connect(url).await.unwrap();
    crate::store::pg_retry::checked(
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}"))).execute(&admin),
    )
    .await
    .unwrap();
    let options = PgConnectOptions::from_str(url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap();
    // The store's own schema: the indexes under test are the ones it creates.
    crate::store::TenantAwarePostgresTaskStore::from_pool(pool.clone())
        .await
        .unwrap();
    // Marked as this release's writes, as the store marks its own, so the
    // earlier-release trigger (`upgrade`) leaves them alone.
    let mut tx = crate::store::pg_retry::checked(pool.begin()).await.unwrap();
    sqlx::query("SET LOCAL a2a.claims = 'on'")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tenant_tasks (tenant_id, id, context_id, state, data, updated_at)
         SELECT 't' || (g % 200), 'task-' || g, 'ctx-' || g,
                CASE WHEN g % 10000 = 0 THEN 'TASK_STATE_WORKING'
                     ELSE 'TASK_STATE_COMPLETED' END,
                jsonb_build_object('id', 'task-' || g, 'pad', repeat('x', 200)),
                now() - g * interval '1 second'
         FROM generate_series(1, $1) g",
    )
    .bind(ROWS)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    crate::store::pg_retry::retry(&crate::metrics::NoopMetrics, || {
        sqlx::query("ANALYZE tenant_tasks").execute(&pool)
    })
    .await
    .unwrap();
    (admin, pool, schema)
}

/// Every node of `plan` that reads `relation`: its node type and index.
fn scans_of(plan: &serde_json::Value, relation: &str, out: &mut Vec<(String, Option<String>)>) {
    match plan {
        serde_json::Value::Array(items) => {
            for item in items {
                scans_of(item, relation, out);
            }
        }
        serde_json::Value::Object(node) => {
            if node.get("Relation Name").and_then(|v| v.as_str()) == Some(relation) {
                out.push((
                    node.get("Node Type")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    node.get("Index Name")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                ));
            }
            for value in node.values() {
                scans_of(value, relation, out);
            }
        }
        _ => {}
    }
}
