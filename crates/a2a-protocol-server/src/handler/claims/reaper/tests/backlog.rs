// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The reaper's scenario in a database that already holds more abandoned
//! tasks than one look takes.
//!
//! A look ends the oldest [`BATCH`](super::super::BATCH) abandoned tasks, so
//! in a database shared with other tests, or with earlier runs that were
//! stopped half way, the scenario's orphan can be behind a full batch of
//! others. The scenario asked for one look and found its orphan still
//! `working`: the PostgreSQL variants failed once in the full suite
//! (2026-11-12, after a day of runs that were killed mid-scenario), the
//! reaper's own history in the database showing exactly 32 tasks ended in
//! that minute where a clean run ends 3. The scenario now looks until the task
//! it asks about is ended.

#![cfg(feature = "sqlite")]

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::task::TaskState;

use super::super::BATCH;
use super::{remove_sqlite, sqlite_age, sqlite_url, task};
use crate::store::TaskStore;

#[tokio::test]
async fn the_reaper_ends_its_orphan_behind_a_backlog_larger_than_one_look() {
    let (url, path) = sqlite_url();
    let store = Arc::new(crate::store::SqliteTaskStore::new(&url).await.unwrap());
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    let claims = store.turn_claims().expect("SQL stores claim turns");
    let age = sqlite_age(pool.clone(), "tasks");
    // Older than anything the scenario writes, and more than a look takes.
    for i in 0..BATCH + 8 {
        let id = format!("backlog-{i}");
        store.save(&task(&id, TaskState::Working)).await.unwrap();
        age(id.clone()).await;
        assert!(
            claims
                .claim(
                    "",
                    &id,
                    "dead",
                    &format!("run-{i}"),
                    Duration::from_millis(1)
                )
                .await
                .unwrap()
        );
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    super::the_reaper_ends_only_what_nobody_runs(store, "", sqlite_age(pool, "tasks")).await;
    remove_sqlite(&path);
}
