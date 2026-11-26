// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! COR-08 (E03 R8), end to end through the handler: a leased child parked at
//! `input-required` whose parent stops renewing is ended by the claim reaper
//! within three lease lifetimes, `canceled` with the cause `lease-lapsed`;
//! one whose parent keeps renewing stays parked. On the in-memory, SQLite
//! and PostgreSQL lease stores.
//!
//! Before COR-08 the watcher left the lease when the turn parked and nothing
//! else read its deadline, so the child stayed `input-required` for good.

use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_types::cancel_cause::CancelCause;
use a2a_protocol_types::task::TaskState;

use super::tests::{reaches, read, start};
use crate::builder::RequestHandlerBuilder;
use crate::handler::MIN_TURN_CLAIM_TTL;
use crate::store::claim::InMemoryTurnClaims;
use crate::store::{InMemoryLeaseStore, InMemoryTaskStore, LeaseStore};
use crate::{EventEmitter, RequestHandler, agent_executor};

/// Asks for input at once.
struct Asks;
agent_executor!(Asks, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::InputRequired)
        .await
});

/// The lease, as long as the shortest claim time to live: the reaper looks
/// once per claim time to live.
const LEASE: Duration = MIN_TURN_CLAIM_TTL;

fn handler(leases: impl LeaseStore) -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Asks)
            .without_agent_card()
            .with_task_store_arc(Arc::new(InMemoryTaskStore::new()))
            .with_turn_claims(InMemoryTurnClaims::new())
            .with_turn_claim_timing(MIN_TURN_CLAIM_TTL, Duration::from_secs(1))
            .with_lease_store(leases)
            .build()
            .expect("handler"),
    )
}

/// A parked child of a parent that stopped renewing ends within three lease
/// lifetimes, and says why.
async fn a_parked_child_of_a_quiet_parent_ends(leases: impl LeaseStore) {
    let h = handler(leases);
    let id = start(&h, Some(LEASE)).await.unwrap();
    reaches(&h, &id, TaskState::InputRequired, Duration::from_secs(5)).await;
    reaches(&h, &id, TaskState::Canceled, 3 * LEASE).await;
    assert_eq!(
        crate::handler::cancel_cause::tests::cause(&h, None, &id).await,
        Some(CancelCause::LeaseLapsed)
    );
}

/// A parked child whose parent renews every third of the lease stays parked
/// past three lease lifetimes.
async fn a_parked_child_of_a_live_parent_stays(leases: impl LeaseStore) {
    let h = handler(leases);
    let id = start(&h, Some(LEASE)).await.unwrap();
    reaches(&h, &id, TaskState::InputRequired, Duration::from_secs(5)).await;
    let began = Instant::now();
    while began.elapsed() < 3 * LEASE {
        assert_eq!(read(&h, &id, true).await, TaskState::InputRequired);
        tokio::time::sleep(LEASE / 3).await;
    }
    assert_eq!(read(&h, &id, false).await, TaskState::InputRequired);
}

#[tokio::test]
async fn in_memory_a_parked_child_of_a_quiet_parent_ends() {
    a_parked_child_of_a_quiet_parent_ends(InMemoryLeaseStore::new()).await;
}

#[tokio::test]
async fn in_memory_a_parked_child_of_a_live_parent_stays() {
    a_parked_child_of_a_live_parent_stays(InMemoryLeaseStore::new()).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_a_parked_child_of_a_quiet_parent_ends() {
    let leases = crate::store::SqliteLeaseStore::new("sqlite::memory:")
        .await
        .unwrap();
    a_parked_child_of_a_quiet_parent_ends(leases).await;
}

#[cfg(feature = "postgres")]
#[cfg(test)]
mod postgres {
    use super::*;
    use crate::store::PostgresLeaseStore;

    /// A schema of its own: the lease contract test empties the shared
    /// `a2a_leases` when it starts, which would take this run's lease.
    async fn own_schema() -> (sqlx::PgPool, String) {
        let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
        let schema = format!("a2a_parked_{}", uuid::Uuid::new_v4().simple());
        let admin = sqlx::PgPool::connect(&url).await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .unwrap();
        let sep = if url.contains('?') { '&' } else { '?' };
        let pool = sqlx::PgPool::connect(&format!("{url}{sep}options=-c%20search_path%3D{schema}"))
            .await
            .unwrap();
        (pool, schema)
    }

    async fn drop_schema(schema: &str) {
        let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
        let admin = sqlx::PgPool::connect(&url).await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin)
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
    async fn postgres_a_parked_child_of_a_quiet_parent_ends() {
        let (pool, schema) = own_schema().await;
        let leases = PostgresLeaseStore::from_pool(pool.clone()).await.unwrap();
        a_parked_child_of_a_quiet_parent_ends(leases).await;
        pool.close().await;
        drop_schema(&schema).await;
    }

    #[tokio::test]
    #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
    async fn postgres_a_parked_child_of_a_live_parent_stays() {
        let (pool, schema) = own_schema().await;
        let leases = PostgresLeaseStore::from_pool(pool.clone()).await.unwrap();
        a_parked_child_of_a_live_parent_stays(leases).await;
        pool.close().await;
        drop_schema(&schema).await;
    }
}
