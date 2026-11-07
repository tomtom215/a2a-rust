// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tenant-aware store's announcements.

use super::*;

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

/// An append that wrote nothing is not announced and says so, as in the plain
/// store: the same event is a replay, another a conflict, and neither is heard
/// before the next real append (the barrier) is.
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn the_tenant_stores_appends_that_wrote_nothing_are_not_announced() {
    let db = Scratch::new("logwake").await;
    let conflicts = Arc::new(Conflicts::default());
    let make = || async {
        TenantAwarePostgresTaskStore::from_pool(db.pool(6).await)
            .await
            .unwrap()
            .with_log_notifications(LogListen::Pool)
    };
    let a = make()
        .await
        .with_metrics(MetricsHandle::new(Arc::clone(&conflicts)));
    let b = make().await;
    let id = TaskId::new("shared");
    for tenant in ["acme", "globex"] {
        TenantContext::scope(tenant, a.save(&task("shared")))
            .await
            .unwrap();
    }
    TenantContext::scope(
        "acme",
        a.append_event(&id, 1, &event("shared", TaskState::Working)),
    )
    .await
    .unwrap();
    let mut raw = PgListener::connect(&db.url).await.unwrap();
    raw.listen(LOG_NOTIFY_CHANNEL).await.unwrap();
    let starting = TenantContext::scope("acme", async { b.watch_event_log(&id) })
        .await
        .expect("watching");
    assert_eq!(woken(starting).await, LogWake::Resynced);
    let again = TenantContext::scope("acme", async { b.watch_event_log(&id) })
        .await
        .expect("watching");

    for (state, counted) in [(TaskState::Working, 0), (TaskState::Failed, 1)] {
        TenantContext::scope("acme", a.append_event(&id, 1, &event("shared", state)))
            .await
            .unwrap();
        assert_eq!(*conflicts.0.lock().unwrap(), counted, "{state:?}");
    }
    TenantContext::scope(
        "globex",
        a.append_event(&id, 1, &event("shared", TaskState::Working)),
    )
    .await
    .unwrap();
    // Heard in order: globex's own append, and nothing for acme's two.
    let barrier = tokio::time::timeout(GUARD, raw.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(barrier.payload(), "6:globexshared");
    assert!(
        again.now_or_never().is_none(),
        "acme's appends wrote nothing"
    );
    db.drop_database().await;
}
