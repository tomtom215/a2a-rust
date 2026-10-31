// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The [`DelegationJournal`] contract (the clauses of
//! `a2a_protocol_types::delegation_journal`), run against PostgreSQL.

use a2a_protocol_types::delegation_journal::InMemoryDelegationJournal;

use super::*;

fn rec(id: &str, owner: &str, at: i64) -> DelegationRecord {
    DelegationRecord::new(id, owner, "http://worker:8080", at)
}

/// Every clause, against any journal. `tag` keeps one run's ids apart from
/// another's in a shared database.
async fn contract(journal: &dyn DelegationJournal, tag: &str) {
    let owner = format!("{tag}-sup");
    begin_and_progress(journal, tag, &owner).await;
    open_and_settle(journal, tag, &owner).await;
    purge(journal, tag, &owner).await;
}

/// begin is idempotent by id (the first record wins); progress sets what is
/// given and never clears what is not; an unknown id is not an error.
async fn begin_and_progress(journal: &dyn DelegationJournal, tag: &str, owner: &str) {
    let id = |n: &str| format!("{tag}-{n}");
    journal
        .begin(&rec(&id("r1"), owner, 10).with_idempotency_key(Some("k1".into())))
        .await
        .unwrap();
    journal
        .begin(&rec(&id("r1"), "other", 99).with_idempotency_key(Some("k2".into())))
        .await
        .unwrap();
    journal
        .progress(&id("r1"), Some("task-1"), None, 20)
        .await
        .unwrap();
    journal
        .progress(&id("r1"), None, Some("evt-7"), 30)
        .await
        .unwrap();
    // An unknown id is not an error, and creates nothing.
    journal
        .progress(&id("purged"), Some("t"), Some("e"), 5)
        .await
        .unwrap();
    journal.settle(&id("purged"), "completed", 6).await.unwrap();
}

/// open lists this owner's unsettled records, oldest first, ties by id; a
/// settled record stops changing and leaves it.
async fn open_and_settle(journal: &dyn DelegationJournal, tag: &str, owner: &str) {
    let id = |n: &str| format!("{tag}-{n}");
    journal.begin(&rec(&id("c"), owner, 30)).await.unwrap();
    journal.begin(&rec(&id("a"), owner, 10)).await.unwrap();
    journal.begin(&rec(&id("done"), owner, 5)).await.unwrap();
    journal
        .begin(&rec(&id("theirs"), &format!("{tag}-someone"), 1))
        .await
        .unwrap();
    journal.settle(&id("done"), "completed", 6).await.unwrap();
    let open = journal.open(owner).await.unwrap();
    let ids: Vec<_> = open.iter().map(|r| r.id.clone()).collect();
    assert_eq!(ids, [id("a"), id("r1"), id("c")], "by start, ties by id");
    let r1 = open.iter().find(|r| r.id == id("r1")).unwrap();
    assert_eq!(r1.owner, owner);
    assert_eq!(r1.idempotency_key.as_deref(), Some("k1"));
    assert_eq!(r1.task_id.as_deref(), Some("task-1"));
    assert_eq!(r1.last_event_id.as_deref(), Some("evt-7"));
    assert_eq!((r1.started_at_ms, r1.updated_at_ms), (10, 30));
    assert!(r1.is_open());

    journal.settle(&id("r1"), "completed", 40).await.unwrap();
    journal.settle(&id("r1"), "lost", 50).await.unwrap();
    journal
        .progress(&id("r1"), Some("late"), Some("late"), 60)
        .await
        .unwrap();
    let after = journal.open(owner).await.unwrap();
    assert!(after.iter().all(|r| r.id != id("r1")));
}

/// purge deletes only what settled before the cutoff; open records never.
async fn purge(journal: &dyn DelegationJournal, tag: &str, owner: &str) {
    let id = |n: &str| format!("{tag}-{n}");
    journal.begin(&rec(&id("old"), owner, 1)).await.unwrap();
    journal.begin(&rec(&id("edge"), owner, 1)).await.unwrap();
    journal.settle(&id("old"), "failed", 1_000).await.unwrap();
    journal.settle(&id("edge"), "failed", 2_000).await.unwrap();
    // Settled at 6 (done), 40 (r1), 1000 (old) and 2000 (edge). A cutoff of
    // 2000 removes those settled before it; "edge" stays.
    let purged = journal.purge_settled(2_000).await.unwrap();
    assert!(purged >= 3, "done, r1 and old at least: {purged}");
    let still_open: Vec<_> = journal
        .open(owner)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(
        still_open,
        [id("a"), id("c")],
        "open records survive a purge"
    );
    assert_eq!(
        journal.purge_settled(2_000).await.unwrap(),
        0,
        "nothing more"
    );
    assert!(
        journal.purge_settled(3_000).await.unwrap() >= 1,
        "edge goes"
    );
}

#[tokio::test]
async fn the_in_memory_journal_keeps_the_same_contract() {
    // The clauses above are the trait's; the types crate's journal is the
    // reference, so this guards the shared contract function itself.
    contract(&InMemoryDelegationJournal::new(), "mem").await;
}

#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn postgres_keeps_the_contract() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let pool = sqlx::postgres::PgPool::connect(&url).await.unwrap();
    let journal = PostgresDelegationJournal::from_pool(pool.clone())
        .await
        .unwrap();
    let tag = format!("t{}", uuid::Uuid::new_v4().simple());
    // What the trait cannot show: a settled record keeps its first outcome and
    // stops changing. Read straight from the table.
    let direct = format!("{tag}-direct");
    journal
        .begin(&rec(&direct, &format!("{tag}-sup"), 1))
        .await
        .unwrap();
    journal
        .progress(&direct, Some("task-1"), Some("evt-1"), 2)
        .await
        .unwrap();
    journal.settle(&direct, "completed", 3).await.unwrap();
    journal.settle(&direct, "lost", 4).await.unwrap();
    journal
        .progress(&direct, Some("late"), Some("late"), 5)
        .await
        .unwrap();
    let row: (Option<String>, Option<String>, Option<String>, i64) = sqlx::query_as(
        "SELECT outcome, task_id, last_event_id, updated_at_ms FROM a2a_delegations WHERE id = $1",
    )
    .bind(&direct)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            Some("completed".into()),
            Some("task-1".into()),
            Some("evt-1".into()),
            3
        )
    );
    contract(&journal, &tag).await;
    // A second pool on the same database sees the same records: the point of
    // keeping them there.
    let other = PostgresDelegationJournal::from_pool(pool).await.unwrap();
    other
        .begin(&rec(&format!("{tag}-x"), &format!("{tag}-sup"), 5))
        .await
        .unwrap();
    assert!(
        journal
            .open(&format!("{tag}-sup"))
            .await
            .unwrap()
            .iter()
            .any(|r| r.id == format!("{tag}-x"))
    );
}
