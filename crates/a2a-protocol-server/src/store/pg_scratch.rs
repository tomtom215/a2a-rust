// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A database of its own for one test: the tables, the notification channels
//! and the sweeps of a store belong to a database, and tests that share one
//! share each other's rows and signals (the mutation job runs every test in
//! its own process, all at once). Test-only.

use sqlx::postgres::{PgPool, PgPoolOptions};

/// A scratch database, dropped by [`drop_database`](Self::drop_database).
pub struct Scratch {
    admin: PgPool,
    name: String,
    /// The URL of the scratch database.
    pub url: String,
}

impl Scratch {
    /// Creates a database named for `prefix` and a fresh id, from
    /// `A2A_TEST_POSTGRES_URL`.
    pub async fn new(prefix: &str) -> Self {
        let admin_url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
        let admin = PgPool::connect(&admin_url).await.expect("admin connect");
        let name = format!("a2a_{prefix}_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .expect("create database");
        let url = format!("{}/{name}", admin_url.rsplit_once('/').expect("path").0);
        Self { admin, name, url }
    }

    /// A pool on the scratch database, `size` connections at most, opened with
    /// the options the task stores' own pools use.
    pub async fn pool(&self, size: u32) -> PgPool {
        PgPoolOptions::new()
            .max_connections(size)
            .connect(&self.url)
            .await
            .expect("scratch pool")
    }

    /// Drops the database, ending any connection still open to it.
    pub async fn drop_database(self) {
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        )))
        .execute(&self.admin)
        .await;
    }
}
