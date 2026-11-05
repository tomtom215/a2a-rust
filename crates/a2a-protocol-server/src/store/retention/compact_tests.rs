// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Live-log compaction (ADR 0047, option C): the cut, and the contract every
//! store keeps, run on each store that can trim.

use std::future::Future;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use super::compact::*;
use crate::store::{AppliedPosition, InMemoryTaskStore, TaskStore};

/// The newest `keep`; never past the snapshot; never the last event.
#[test]
fn the_cut_keeps_the_newest_and_never_passes_the_snapshot_or_the_end() {
    assert_eq!(trim_point(100, 100, 10), 91);
    assert_eq!(
        trim_point(100, 50, 10),
        51,
        "not past what the snapshot holds"
    );
    assert_eq!(
        trim_point(100, 100, 100),
        1,
        "a log of exactly keep is whole"
    );
    assert_eq!(trim_point(5, 5, 10), 1, "a short log is whole");
    assert_eq!(trim_point(100, 100, 0), 100, "keep 0 keeps the last event");
    assert_eq!(trim_point(u64::MAX, u64::MAX, 1), u64::MAX);
}

#[test]
fn the_sql_count_is_keep_floored_at_one() {
    assert_eq!(over(10), 10);
    assert_eq!(over(0), 1);
    assert_eq!(over(u64::MAX), i64::MAX);
}

fn status(task: &str, state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new(task),
        ContextId::new("c"),
        TaskStatus::new(state),
    ))
}

/// Saves `id` in `state` (with its snapshot at `applied`, if any) and
/// appends events `from..=to` to its log.
async fn grow<S: TaskStore + ?Sized>(
    store: &S,
    id: &str,
    state: TaskState,
    applied: Option<u64>,
    events: std::ops::RangeInclusive<u64>,
) {
    let task = Task::new(TaskId::new(id), ContextId::new("c"), TaskStatus::new(state));
    match applied {
        Some(seq) => AppliedPosition::scope(seq, store.save(&task)).await,
        None => store.save(&task).await,
    }
    .unwrap();
    for seq in events {
        store
            .append_event(&TaskId::new(id), seq, &status(id, TaskState::Working))
            .await
            .unwrap();
    }
}

async fn positions<S: TaskStore + ?Sized>(store: &S, id: &str) -> Vec<u64> {
    let events = store
        .read_events(&TaskId::new(id), 0, usize::MAX)
        .await
        .unwrap();
    events.iter().map(|event| event.seq).collect()
}

/// What every store's compaction keeps to, with `compact` running one sweep
/// at `keep` = 10 over the store and returning the events it dropped.
pub async fn check_compaction<S, F, Fut>(store: &S, compact: F)
where
    S: TaskStore + ?Sized,
    F: Fn() -> Fut,
    Fut: Future<Output = A2aResult<u64>>,
{
    // A live task whose snapshot reflects its whole log: the newest ten stay.
    grow(store, "live", TaskState::Working, Some(100), 1..=100).await;
    // Its snapshot reflects only the first fifty: nothing after them goes.
    grow(store, "behind", TaskState::Working, Some(50), 1..=100).await;
    // Terminal: retention purges it whole, compaction leaves it alone.
    grow(store, "done", TaskState::Completed, Some(100), 1..=100).await;
    // No position stored: what the snapshot holds is unknown, so nothing goes.
    grow(store, "unpositioned", TaskState::Working, None, 1..=100).await;
    // At the bound: nothing to do.
    grow(store, "short", TaskState::Working, Some(10), 1..=10).await;

    assert_eq!(compact().await.unwrap(), 90 + 50);
    assert_eq!(
        positions(store, "live").await,
        (91..=100).collect::<Vec<_>>()
    );
    assert_eq!(
        positions(store, "behind").await,
        (51..=100).collect::<Vec<_>>()
    );
    for whole in ["done", "unpositioned"] {
        assert_eq!(positions(store, whole).await.len(), 100, "{whole}");
    }
    assert_eq!(positions(store, "short").await.len(), 10);

    // The resume contract: from before the cut the log does not cover the
    // reader, who is answered with the snapshot; from the cut it does.
    let live = TaskId::new("live");
    assert_eq!(store.earliest_event_seq(&live).await.unwrap(), Some(91));
    assert_eq!(store.last_event_seq(&live).await.unwrap(), 100);
    assert!(!store.event_log_covers(&live, 89).await.unwrap());
    assert!(store.event_log_covers(&live, 90).await.unwrap());

    // A task that keeps producing events keeps a bounded log: after every
    // sweep it holds the newest ten, however many it has written.
    for round in 1..=5u64 {
        let end = 100 + round * 50;
        grow(store, "live", TaskState::Working, Some(end), end - 49..=end).await;
        assert_eq!(compact().await.unwrap(), 50, "round {round}");
        assert_eq!(
            positions(store, "live").await,
            (end - 9..=end).collect::<Vec<_>>()
        );
    }
}

/// The tenant-aware in-memory store trims the partition in scope, and asks
/// nothing of a partition that does not exist.
#[tokio::test]
async fn the_tenant_in_memory_store_keeps_the_compaction_contract() {
    use crate::store::tenant::TenantContext;
    let store = crate::store::TenantAwareInMemoryTaskStore::new();
    TenantContext::scope(
        "acme",
        check_compaction(&store, || async {
            let mut dropped = 0;
            for id in ["live", "behind", "done", "unpositioned", "short"] {
                dropped += compact_task(&store, &TaskId::new(id), 10).await?;
            }
            Ok(dropped)
        }),
    )
    .await;
    let live = TaskId::new("live");
    let elsewhere = TenantContext::scope("nobody", store.trim_event_log(&live, 99));
    assert_eq!(elsewhere.await.unwrap(), 0);
    let kept = TenantContext::scope("acme", store.earliest_event_seq(&live));
    assert_eq!(
        kept.await.unwrap(),
        Some(341),
        "the other partition's trim left acme alone"
    );
}

/// The in-memory store, compacted task by task: its own bound,
/// `TaskStoreConfig::max_events_per_task`, is far above these logs.
#[tokio::test]
async fn the_in_memory_store_keeps_the_compaction_contract() {
    let store = InMemoryTaskStore::new();
    check_compaction(&store, || async {
        let mut dropped = 0;
        for id in ["live", "behind", "done", "unpositioned", "short"] {
            dropped += compact_task(&store, &TaskId::new(id), 10).await?;
        }
        Ok(dropped)
    })
    .await;
    assert_eq!(
        compact_task(&store, &TaskId::new("gone"), 10)
            .await
            .unwrap(),
        0
    );
}

#[cfg(feature = "sqlite")]
mod sqlite {
    use std::time::Duration;

    use super::check_compaction;
    use crate::store::retention::RetentionPolicy;
    use crate::store::tenant::TenantContext;
    use crate::store::{SqliteTaskStore, TenantAwareSqliteTaskStore};

    const fn policy() -> RetentionPolicy {
        RetentionPolicy::new(Duration::from_hours(365 * 24)).with_max_events_per_live_task(Some(10))
    }

    #[tokio::test]
    async fn the_sqlite_store_compacts_live_logs_in_its_sweep() {
        let store = SqliteTaskStore::new("sqlite::memory:").await.unwrap();
        check_compaction(&store, || async {
            Ok(store.purge_expired(&policy()).await?.events_compacted)
        })
        .await;
        let off = RetentionPolicy::new(Duration::from_hours(365 * 24));
        assert_eq!(store.purge_expired(&off).await.unwrap().events_compacted, 0);
    }

    /// The sweep runs outside any request and scopes each task's partition
    /// itself.
    #[tokio::test]
    async fn the_tenant_sqlite_store_compacts_each_partition() {
        let store = TenantAwareSqliteTaskStore::new("sqlite::memory:")
            .await
            .unwrap();
        // The sweep runs from another tenant's scope: it must find each
        // task's partition itself.
        TenantContext::scope(
            "acme",
            check_compaction(&store, || {
                TenantContext::scope("elsewhere", async {
                    Ok(store.purge_expired(&policy()).await?.events_compacted)
                })
            }),
        )
        .await;
    }
}

#[cfg(feature = "postgres")]
#[cfg(test)]
mod postgres {
    use std::time::Duration;

    use super::check_compaction;
    use crate::store::retention::RetentionPolicy;
    use crate::store::tenant::TenantContext;
    use crate::store::{PostgresTaskStore, TenantAwarePostgresTaskStore};

    const fn policy() -> RetentionPolicy {
        RetentionPolicy::new(Duration::from_hours(365 * 24)).with_max_events_per_live_task(Some(10))
    }

    /// A URL whose connections use a schema of their own, so the sweep sees
    /// only this test's tasks in a database other suites share.
    async fn own_schema() -> String {
        let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
        let schema = format!("compact_{}", uuid::Uuid::new_v4().simple());
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        // The name is a fresh UUID's digits: nothing a caller wrote.
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&pool)
            .await
            .unwrap();
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}options=-c%20search_path%3D{schema}")
    }

    #[tokio::test]
    #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
    async fn the_postgres_store_compacts_live_logs_in_its_sweep() {
        let store = PostgresTaskStore::new(&own_schema().await).await.unwrap();
        check_compaction(&store, || async {
            Ok(store.purge_expired(&policy()).await?.events_compacted)
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
    async fn the_tenant_postgres_store_compacts_each_partition() {
        let store = TenantAwarePostgresTaskStore::new(&own_schema().await)
            .await
            .unwrap();
        // The sweep runs from another tenant's scope: it must find each
        // task's partition itself.
        TenantContext::scope(
            "acme",
            check_compaction(&store, || {
                TenantContext::scope("elsewhere", async {
                    Ok(store.purge_expired(&policy()).await?.events_compacted)
                })
            }),
        )
        .await;
    }
}
