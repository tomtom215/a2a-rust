// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `PostgreSQL` task stores announce an event-log append and wake the
//! streams waiting on it (audit E-F25 (b), ADR 0057), against a live database.
//! A database of its own per test: the channel belongs to a database.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
use futures_util::FutureExt as _;
use sqlx::postgres::PgListener;

use crate::metrics::{Metrics, MetricsHandle};
use crate::store::tests::pg_scratch::Scratch;
use crate::store::{
    LOG_NOTIFY_CHANNEL, LogListen, LogWake, LogWatch, PostgresTaskStore, TaskStore,
    TenantAwarePostgresTaskStore, TenantContext,
};

/// A deadline only a lost wake-up reaches; a pass never waits on it.
const GUARD: Duration = Duration::from_secs(15);

fn event(task: &str, state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new(task),
        ContextId::new("c"),
        TaskStatus::new(state),
    ))
}

fn task(id: &str) -> Task {
    Task::new(
        TaskId::new(id),
        ContextId::new("c"),
        TaskStatus::new(TaskState::InputRequired),
    )
}

async fn woken(watch: LogWatch) -> LogWake {
    tokio::time::timeout(GUARD, watch)
        .await
        .expect("the waiter was woken")
}

/// A store that announces (listening on `listen`) over its own pool.
async fn announcing(db: &Scratch, listen: LogListen) -> PostgresTaskStore {
    PostgresTaskStore::from_pool(db.pool(6).await)
        .await
        .unwrap()
        .with_log_notifications(listen)
}

/// A watch on `id`, past the wake the listener's start gives every waiter.
async fn watch_when_listening(store: &impl TaskStore, id: &str) -> LogWatch {
    let id = TaskId::new(id);
    let starting = store.watch_event_log(&id).expect("watching");
    assert_eq!(
        woken(starting).await,
        LogWake::Resynced,
        "the listener is up"
    );
    store.watch_event_log(&id).expect("watching")
}

/// What a store reports when an append lands on a position it holds.
#[derive(Default)]
struct Conflicts(Mutex<u32>);

impl Metrics for Conflicts {
    fn on_persistence_error(&self, operation: &str, kind: &str) {
        if (operation, kind) == ("event_append", "position_conflict") {
            *self.0.lock().unwrap() += 1;
        }
    }
}

// ── What is announced, and to whom ───────────────────────────────────────────

/// An append on one store wakes a watch on another, for that task only: every
/// notification is read in order, so a watch on the other task that is still
/// pending when its own has been heard was not named.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn an_append_wakes_the_watch_of_its_task_on_another_store() {
    let db = Scratch::new("logwake").await;
    let (a, b) = (
        announcing(&db, LogListen::Pool).await,
        announcing(&db, LogListen::Pool).await,
    );
    for id in ["t1", "t2"] {
        a.save(&task(id)).await.unwrap();
    }
    let t1 = watch_when_listening(&b, "t1").await;
    let t2 = b.watch_event_log(&TaskId::new("t2")).expect("watching");
    a.append_event(&TaskId::new("t2"), 1, &event("t2", TaskState::Working))
        .await
        .unwrap();
    assert_eq!(woken(t2).await, LogWake::Notified);
    assert!(t1.now_or_never().is_none(), "t1 was not appended to");
    db.drop_database().await;
}

/// A replayed append, a position already held, is not announced and says it
/// wrote nothing: the same event again is a replay and counts nothing, another
/// event at the position is a conflict and counts one. (`rows_affected` of the
/// announcing statement is the rows inserted, as the plain insert's is.)
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn an_append_that_wrote_nothing_is_not_announced_and_is_still_seen_as_such() {
    let db = Scratch::new("logwake").await;
    let conflicts = Arc::new(Conflicts::default());
    let a = announcing(&db, LogListen::Pool)
        .await
        .with_metrics(MetricsHandle::new(Arc::clone(&conflicts)));
    let b = announcing(&db, LogListen::Pool).await;
    for id in ["t1", "t2"] {
        a.save(&task(id)).await.unwrap();
    }
    let t1 = TaskId::new("t1");
    a.append_event(&t1, 1, &event("t1", TaskState::Working))
        .await
        .unwrap();
    let again = watch_when_listening(&b, "t1").await;
    let barrier = b.watch_event_log(&TaskId::new("t2")).expect("watching");

    a.append_event(&t1, 1, &event("t1", TaskState::Working))
        .await
        .unwrap();
    assert_eq!(
        *conflicts.0.lock().unwrap(),
        0,
        "the same event is a replay"
    );
    a.append_event(&t1, 1, &event("t1", TaskState::Failed))
        .await
        .unwrap();
    assert_eq!(
        *conflicts.0.lock().unwrap(),
        1,
        "another event is a conflict"
    );

    // Neither woke the watch on t1: a later append to t2 is heard first.
    a.append_event(&TaskId::new("t2"), 1, &event("t2", TaskState::Working))
        .await
        .unwrap();
    assert_eq!(woken(barrier).await, LogWake::Notified);
    assert!(again.now_or_never().is_none());
    assert_eq!(a.read_events(&t1, 0, 10).await.unwrap().len(), 1);
    db.drop_database().await;
}

/// The statement itself: one row inserted is one affected, and none is none,
/// whether or not it announces.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn the_announcing_statement_reports_the_rows_it_inserted() {
    use crate::store::postgres_store::event_log::{APPEND_NOTIFY_SQL, APPEND_SQL};
    let db = Scratch::new("logwake").await;
    let store = announcing(&db, LogListen::Pool).await;
    store.save(&task("t1")).await.unwrap();
    let pool = db.pool(2).await;
    let payload = serde_json::json!({"k": 1});
    let run = |sql: &'static str, position: i64| {
        let pool = pool.clone();
        let payload = payload.clone();
        async move {
            let query = sqlx::query(sql)
                .bind("t1")
                .bind(position)
                .bind(payload)
                .bind(None::<String>);
            let query = if sql == APPEND_NOTIFY_SQL {
                query.bind(LOG_NOTIFY_CHANNEL).bind("0:t1")
            } else {
                query
            };
            query.execute(&pool).await.unwrap().rows_affected()
        }
    };
    assert_eq!(run(APPEND_NOTIFY_SQL, 1).await, 1, "inserted");
    assert_eq!(run(APPEND_NOTIFY_SQL, 1).await, 0, "the position is held");
    assert_eq!(
        run(APPEND_SQL, 2).await,
        1,
        "the plain insert, for comparison"
    );
    assert_eq!(run(APPEND_SQL, 2).await, 0);
    db.drop_database().await;
}

/// A task whose key does not fit a notification is appended, not announced and
/// not watched: its stream polls. A store that did not opt in watches nothing.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn an_oversized_task_id_is_appended_but_neither_announced_nor_watched() {
    let db = Scratch::new("logwake").await;
    let store = announcing(&db, LogListen::Pool).await;
    let long = "x".repeat(600);
    store.save(&task(&long)).await.unwrap();
    let id = TaskId::new(long.as_str());
    assert!(store.watch_event_log(&id).is_none(), "polled, not watched");
    store
        .append_event(&id, 1, &event(&long, TaskState::Working))
        .await
        .unwrap();
    assert_eq!(store.read_events(&id, 0, 10).await.unwrap().len(), 1);

    let plain = PostgresTaskStore::from_pool(db.pool(2).await)
        .await
        .unwrap();
    assert!(plain.watch_event_log(&TaskId::new("t1")).is_none());
    db.drop_database().await;
}

/// The wire: a store that opted in sends exactly one notification per
/// inserted row, on the channel, with the task's key; a store that did not
/// sends none. A raw listener reads them.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn only_an_opted_in_store_notifies_and_the_payload_is_the_key() {
    let db = Scratch::new("logwake").await;
    let plain = PostgresTaskStore::from_pool(db.pool(2).await)
        .await
        .unwrap();
    let announcing = announcing(&db, LogListen::Pool).await;
    for id in ["quiet", "loud"] {
        plain.save(&task(id)).await.unwrap();
    }
    let mut raw = PgListener::connect(&db.url).await.unwrap();
    raw.listen(LOG_NOTIFY_CHANNEL).await.unwrap();

    plain
        .append_event(
            &TaskId::new("quiet"),
            1,
            &event("quiet", TaskState::Working),
        )
        .await
        .unwrap();
    announcing
        .append_event(&TaskId::new("loud"), 1, &event("loud", TaskState::Working))
        .await
        .unwrap();
    let first = tokio::time::timeout(GUARD, raw.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.channel(), LOG_NOTIFY_CHANNEL);
    assert_eq!(first.payload(), "0:loud", "the plain store sent nothing");
    db.drop_database().await;
}

/// The tenant store announces the partition it stores under, and a watch is on
/// that partition's task only.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn the_tenant_store_announces_its_partition() {
    let db = Scratch::new("logwake").await;
    let make = || async {
        TenantAwarePostgresTaskStore::from_pool(db.pool(6).await)
            .await
            .unwrap()
            .with_log_notifications(LogListen::Pool)
    };
    let (a, b) = (make().await, make().await);
    let id = TaskId::new("shared");
    for tenant in ["acme", "globex"] {
        TenantContext::scope(tenant, a.save(&task("shared")))
            .await
            .unwrap();
    }
    let mut raw = PgListener::connect(&db.url).await.unwrap();
    raw.listen(LOG_NOTIFY_CHANNEL).await.unwrap();

    let starting = TenantContext::scope("acme", async { b.watch_event_log(&id) })
        .await
        .expect("watching");
    assert_eq!(woken(starting).await, LogWake::Resynced);
    let acme = TenantContext::scope("acme", async { b.watch_event_log(&id) })
        .await
        .expect("watching");
    let globex = TenantContext::scope("globex", async { b.watch_event_log(&id) })
        .await
        .expect("watching");
    TenantContext::scope(
        "acme",
        a.append_event(&id, 1, &event("shared", TaskState::Working)),
    )
    .await
    .unwrap();
    assert_eq!(woken(acme).await, LogWake::Notified);
    assert!(globex.now_or_never().is_none(), "another tenant's task");
    let heard = tokio::time::timeout(GUARD, raw.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(heard.payload(), "4:acmeshared");
    db.drop_database().await;
}

// ── The listener ─────────────────────────────────────────────────────────────

/// A listener on a connection of its own, to a URL, hears what the pool's
/// stores announce.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn a_direct_listener_hears_what_another_store_announces() {
    let db = Scratch::new("logwake").await;
    let a = announcing(&db, LogListen::Pool).await;
    let b = announcing(&db, LogListen::Direct(db.url.clone())).await;
    a.save(&task("t1")).await.unwrap();
    let watch = watch_when_listening(&b, "t1").await;
    a.append_event(&TaskId::new("t1"), 1, &event("t1", TaskState::Working))
        .await
        .unwrap();
    assert_eq!(woken(watch).await, LogWake::Notified);
    db.drop_database().await;
}

/// A lost connection wakes every waiter to look, and the listener is back: the
/// next append is heard.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn a_lost_listener_connection_wakes_every_waiter_and_recovers() {
    let db = Scratch::new("logwake").await;
    let (a, b) = (
        announcing(&db, LogListen::Pool).await,
        announcing(&db, LogListen::Direct(db.url.clone())).await,
    );
    a.save(&task("t1")).await.unwrap();
    let waiting = watch_when_listening(&b, "t1").await;
    let admin = db.pool(2).await;
    let ended: Vec<(bool,)> = sqlx::query_as(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE datname = current_database() AND query ILIKE 'LISTEN%'",
    )
    .fetch_all(&admin)
    .await
    .unwrap();
    assert!(!ended.is_empty(), "found the listener's backend");
    assert_eq!(woken(waiting).await, LogWake::Resynced);

    let after = b.watch_event_log(&TaskId::new("t1")).expect("watching");
    // Announced once the listener is listening again; retried until it is.
    let mut seq = 0;
    let heard = tokio::time::timeout(GUARD, async {
        tokio::pin!(after);
        loop {
            seq += 1;
            a.append_event(&TaskId::new("t1"), seq, &event("t1", TaskState::Working))
                .await
                .unwrap();
            if let Some(wake) = (&mut after).now_or_never() {
                return wake;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the listener came back");
    assert!(matches!(heard, LogWake::Notified | LogWake::Resynced));
    db.drop_database().await;
}

/// A store's `Debug` form does not print the URL it listens on, and the
/// listener's two forms are named.
#[test]
fn a_listen_setting_prints_without_its_url() {
    // The channel is named in the book for operators who watch it.
    assert_eq!(LOG_NOTIFY_CHANNEL, "a2a_task_log");
    assert_eq!(format!("{:?}", LogListen::Pool), "Pool");
    assert_eq!(
        format!(
            "{:?}",
            LogListen::Direct("postgres://u:secret@h/d".to_owned())
        ),
        "Direct(<url>)"
    );
}

/// Outside a runtime the listener cannot run, so nothing is watched and the
/// stream polls.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn a_watch_needs_a_runtime_to_listen_in() {
    let db = Scratch::new("logwake").await;
    let store = announcing(&db, LogListen::Pool).await;
    let id = TaskId::new("t1");
    let outside = std::thread::spawn({
        let store = store.clone();
        move || store.watch_event_log(&id).is_none()
    })
    .join()
    .unwrap();
    assert!(outside, "no runtime on that thread");
    db.drop_database().await;
}
