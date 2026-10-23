// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Kill-switch conformance (`conformance::halt`) for every halt store this
//! crate ships, through the public API an adopter calls.
//!
//! The PostgreSQL case is `#[ignore]`d without a live server:
//!
//! ```bash
//! A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
//!   cargo test -p a2a-protocol-server --features conformance,postgres \
//!   --test kill_switch_conformance -- --include-ignored
//! ```
#![cfg(feature = "conformance")]

use std::sync::Arc;

use a2a_protocol_server::conformance::halt::{check_handlers, check_store};
use a2a_protocol_server::store::{HaltStore, InMemoryHaltStore};

#[tokio::test]
async fn the_in_memory_halt_store_conforms() {
    let store = InMemoryHaltStore::new();
    let open = move || {
        let store = store.clone();
        async move { Arc::new(store) as Arc<dyn HaltStore> }
    };
    check_store(&open).await.assert_pass();
    check_handlers(&open).await.assert_pass();
}

/// SQLite does not push, so its two push checks are not applicable and
/// replicas learn of a halt by polling; one file, opened afresh by each
/// handle, stands for a restart.
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn the_sqlite_halt_store_conforms() {
    use a2a_protocol_server::store::SqliteHaltStore;
    let path = std::env::temp_dir().join(format!("a2a-halt-conformance-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let open = move || {
        let url = url.clone();
        async move { Arc::new(SqliteHaltStore::new(&url).await.expect("open")) as Arc<dyn HaltStore> }
    };
    let store = check_store(&open).await;
    store.assert_pass();
    assert_eq!(
        store.results().len() - store.graded(),
        2,
        "the push checks: {store}"
    );
    check_handlers(&open).await.assert_pass();
    let _ = std::fs::remove_file(&path);
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs A2A_TEST_POSTGRES_URL"]
async fn the_postgres_halt_store_conforms() {
    use a2a_protocol_server::store::PostgresHaltStore;
    let admin_url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let admin = sqlx::postgres::PgPool::connect(&admin_url)
        .await
        .expect("connect");
    let name = format!("a2a_halt_conformance_{}", std::process::id());
    let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .expect("create database");
    let url = format!("{}/{name}", admin_url.rsplit_once('/').expect("path").0);
    let open = move || {
        let url = url.clone();
        async move {
            let pool = sqlx::postgres::PgPool::connect(&url)
                .await
                .expect("connect");
            Arc::new(PostgresHaltStore::from_pool(pool).await.expect("open")) as Arc<dyn HaltStore>
        }
    };
    check_store(&open).await.assert_pass();
    check_handlers(&open).await.assert_pass();
    let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await;
}
