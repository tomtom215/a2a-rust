// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The journal contract, held by every store: a write is accepted only as the
//! successor of the revision stored, a checkpoint comes back as it went in, and
//! keys are independent.

use std::collections::{BTreeMap, HashMap};

use crate::tool_loop::journal::{
    ActionReceipt, CHECKPOINT_VERSION, FinalAnswer, InMemoryStepJournal, JournalEntry,
    JournalError, PendingApproval, Pins, ReceiptState, RunCheckpoint, RunOwner, StepJournal,
    to_text,
};
use crate::tool_loop::model::{ChatMessage, ToolCall};

/// A checkpoint that uses every variant and field.
fn checkpoint(revision: u64) -> RunCheckpoint {
    let call = ToolCall::new("c1", "transfer", r#"{"a":1}"#);
    RunCheckpoint {
        version: CHECKPOINT_VERSION,
        revision,
        pins: Pins {
            model: "m".to_owned(),
            preamble: "sha256:aa".to_owned(),
            tools: "sha256:bb".to_owned(),
            modes: "sha256:cc".to_owned(),
            labels: BTreeMap::from([("policy".to_owned(), "v1".to_owned())]),
        },
        turn: 2,
        recovered: true,
        messages: vec![
            ChatMessage::System("be careful".to_owned()),
            ChatMessage::User("go".to_owned()),
            ChatMessage::Assistant {
                text: "calling".to_owned(),
                tool_calls: vec![call.clone()],
            },
            ChatMessage::ToolResult {
                call_id: "c1".to_owned(),
                name: "transfer".to_owned(),
                content: "ok".to_owned(),
            },
        ],
        trace: vec!["transfer [c1]: ran".to_owned()],
        done: HashMap::from([("k".to_owned(), "v".to_owned())]),
        receipts: vec![ActionReceipt {
            call_id: "c1".to_owned(),
            tool: "transfer".to_owned(),
            arguments: "sha256:dd".to_owned(),
            state: ReceiptState::AwaitingApproval,
        }],
        pending_approval: Some(PendingApproval {
            call,
            digest: "sha256:ee".to_owned(),
            summary: "move money".to_owned(),
            principal: Some("alice".to_owned()),
            tenant: Some("acme".to_owned()),
            task: Some("t-1".to_owned()),
        }),
        answer: Some(FinalAnswer {
            text: "done".to_owned(),
            finish_reason: Some("stop".to_owned()),
        }),
    }
}

fn owner(principal: &str, tenant: &str) -> RunOwner {
    RunOwner::new(
        Some(tenant.to_owned()),
        Some(principal.to_owned()),
        Some("t-1".to_owned()),
    )
}

/// The entry of revision `revision`, Alice's in `acme`, written at
/// `revision * 1000` ms.
fn entry(revision: u64) -> JournalEntry {
    entry_of(
        revision,
        owner("alice", "acme"),
        false,
        i64::try_from(revision).unwrap() * 1000,
    )
}

fn entry_of(revision: u64, who: RunOwner, completed: bool, at: i64) -> JournalEntry {
    JournalEntry::new(
        revision,
        who,
        completed,
        at,
        to_text(&checkpoint(revision)).unwrap(),
    )
}

async fn contract(journal: &dyn StepJournal) {
    let key = format!("run-{}", uuid::Uuid::new_v4());
    assert_eq!(journal.load(&key).await, Ok(None));

    // The first write is revision 1, and only that.
    assert_eq!(
        journal.save(&key, &entry(2)).await,
        Err(JournalError::Conflict),
        "a first write that skips revision 1"
    );
    assert_eq!(journal.save(&key, &entry(1)).await, Ok(()));
    assert_eq!(
        journal.save(&key, &entry(1)).await,
        Err(JournalError::Conflict),
        "the same revision twice"
    );
    assert_eq!(
        journal.save(&key, &entry(3)).await,
        Err(JournalError::Conflict),
        "a revision skipped"
    );
    assert_eq!(journal.save(&key, &entry(2)).await, Ok(()));
    let back = journal.load(&key).await.unwrap().unwrap();
    assert_eq!(back, entry(2), "an entry comes back as it went in");
    assert_eq!(back.checkpoint(), Ok(checkpoint(2)));

    // Keys are independent.
    let other = format!("run-{}", uuid::Uuid::new_v4());
    assert_eq!(journal.save(&other, &entry(1)).await, Ok(()));
    assert_eq!(journal.load(&key).await, Ok(Some(entry(2))));

    // A clear is fenced: a stale revision removes nothing (N04).
    assert_eq!(journal.clear(&key, 1).await, Err(JournalError::Conflict));
    assert_eq!(journal.load(&key).await, Ok(Some(entry(2))));
    journal.clear(&key, 2).await.unwrap();
    assert_eq!(journal.load(&key).await, Ok(None));
    assert_eq!(journal.load(&other).await, Ok(Some(entry(1))));
    // A cleared key starts again at revision 1, and a stale writer that
    // held revision 2 cannot write over it.
    assert_eq!(journal.save(&key, &entry(1)).await, Ok(()));
    assert_eq!(
        journal.save(&key, &entry(3)).await,
        Err(JournalError::Conflict)
    );
    // Clearing what is not there is not an error.
    journal.clear("never-written", 7).await.unwrap();
    journal.clear(&key, 1).await.unwrap();
    journal.clear(&other, 1).await.unwrap();

    lifecycle(journal).await;
}

/// Who a run is for, and how long it is kept (N07).
async fn lifecycle(journal: &dyn StepJournal) {
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let (alice, bob) = (format!("alice-{tag}"), format!("bob-{tag}"));
    let k = |n: &str| format!("{n}-{tag}");
    // Alice: a finished run in acme at 1 s, an unfinished one in acme at 5 s,
    // one in globex at 9 s; Bob: one in acme at 1 s.
    journal
        .save(&k("a1"), &entry_of(1, owner(&alice, "acme"), true, 1_000))
        .await
        .unwrap();
    journal
        .save(&k("a2"), &entry_of(1, owner(&alice, "acme"), false, 5_000))
        .await
        .unwrap();
    journal
        .save(
            &k("a3"),
            &entry_of(1, owner(&alice, "globex"), false, 9_000),
        )
        .await
        .unwrap();
    journal
        .save(&k("b1"), &entry_of(1, owner(&bob, "acme"), false, 1_000))
        .await
        .unwrap();

    let keys = |runs: Vec<crate::tool_loop::RunSummary>| -> Vec<String> {
        runs.into_iter().map(|r| r.key).collect()
    };
    assert_eq!(
        keys(journal.runs_of(&alice, None).await.unwrap()),
        vec![k("a1"), k("a2"), k("a3")]
    );
    let in_acme = journal.runs_of(&alice, Some("acme")).await.unwrap();
    assert_eq!(keys(in_acme.clone()), vec![k("a1"), k("a2")]);
    assert!(in_acme[0].completed && !in_acme[1].completed);
    assert_eq!(in_acme[1].updated_at_ms, 5_000);
    assert_eq!(in_acme[0].owner, owner(&alice, "acme"));

    // The sweep keeps what is newer than each cutoff: finished runs before
    // 2 s go; unfinished ones before 2 s go (Bob's), Alice's at 5 s stays.
    // Other tests' rows written now are far newer than either cutoff.
    let swept = journal.sweep(2_000, 2_000).await.unwrap();
    assert!(swept >= 2, "{swept}");
    assert_eq!(
        journal.load(&k("a1")).await,
        Ok(None),
        "a finished run past the window"
    );
    assert_eq!(
        journal.load(&k("b1")).await,
        Ok(None),
        "an unfinished run past the window"
    );
    assert!(journal.load(&k("a2")).await.unwrap().is_some());

    // Erasure in one tenant, then in all.
    assert_eq!(journal.erase(&alice, Some("acme")).await, Ok(1));
    assert_eq!(
        keys(journal.runs_of(&alice, None).await.unwrap()),
        vec![k("a3")]
    );
    assert_eq!(journal.erase(&alice, None).await, Ok(1));
    assert_eq!(journal.runs_of(&alice, None).await, Ok(Vec::new()));
    assert_eq!(journal.erase(&alice, None).await, Ok(0));
}

#[tokio::test]
async fn the_in_memory_journal_keeps_the_contract() {
    let journal = InMemoryStepJournal::new();
    assert!(journal.is_empty());
    let finished = entry_of(1, owner("carol", "acme"), true, i64::MAX);
    journal.save("finished", &finished).await.unwrap();
    assert_eq!((journal.len(), journal.unfinished()), (1, 0));
    assert!(!journal.is_empty());
    contract(&journal).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn the_sqlite_journal_keeps_the_contract() {
    let journal = crate::tool_loop::SqliteStepJournal::new("sqlite::memory:")
        .await
        .unwrap();
    contract(&journal).await;
}

/// A file database outlives the journal that wrote it: a new connection reads
/// what the old one stored, which is what a restart is.
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn the_sqlite_journal_survives_a_restart() {
    let path = std::env::temp_dir().join(format!("a2a-journal-{}.db", uuid::Uuid::new_v4()));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let first = crate::tool_loop::SqliteStepJournal::new(&url)
        .await
        .unwrap();
    first.save("run", &entry(1)).await.unwrap();
    drop(first);
    let second = crate::tool_loop::SqliteStepJournal::new(&url)
        .await
        .unwrap();
    assert_eq!(second.load("run").await, Ok(Some(entry(1))));
    assert_eq!(second.save("run", &entry(2)).await, Ok(()));
    let _ = std::fs::remove_file(&path);
}

/// An unreadable body is an error, not an absent run: resuming from nothing
/// would repeat what the lost checkpoint recorded.
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn an_unreadable_checkpoint_is_an_error_and_not_a_missing_run() {
    let pool = crate::sqlite_pool::sqlite_pool("sqlite::memory:")
        .await
        .unwrap();
    let journal = crate::tool_loop::SqliteStepJournal::from_pool(pool.clone())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO a2a_run_journal (run_key, revision, body, updated_at) VALUES ('r', 1, 'not json', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let stored = journal.load("r").await.unwrap().expect("the row");
    assert!(matches!(stored.checkpoint(), Err(JournalError::Store(_))));
}

/// A table made by the earlier build (no owner, no `completed`) is given the
/// new columns, and its rows read as unfinished and ownerless.
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn a_table_from_before_the_owner_columns_is_migrated() {
    let pool = crate::sqlite_pool::sqlite_pool("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE a2a_run_journal (run_key TEXT PRIMARY KEY, revision INTEGER NOT NULL,
         body TEXT NOT NULL, updated_at INTEGER NOT NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO a2a_run_journal VALUES ('old', 1, '{}', 5)")
        .execute(&pool)
        .await
        .unwrap();
    let journal = crate::tool_loop::SqliteStepJournal::from_pool(pool.clone())
        .await
        .unwrap();
    // Twice: the migration is idempotent.
    let journal2 = crate::tool_loop::SqliteStepJournal::from_pool(pool)
        .await
        .unwrap();
    let old = journal2.load("old").await.unwrap().unwrap();
    assert_eq!(
        (old.revision, old.completed, old.owner),
        (1, false, RunOwner::default())
    );
    contract(&journal).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn the_postgres_journal_keeps_the_contract() {
    let admin_url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let admin = sqlx::postgres::PgPool::connect(&admin_url).await.unwrap();
    let name = format!("a2a_journal_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .unwrap();
    let url = format!("{}/{name}", admin_url.rsplit_once('/').expect("path").0);
    let pool = sqlx::postgres::PgPool::connect(&url).await.unwrap();
    let journal = crate::tool_loop::PostgresStepJournal::from_pool(pool)
        .await
        .unwrap();
    // It names itself, and shows no pool or metrics handle.
    let shown = format!("{journal:?}");
    assert!(shown.starts_with("PostgresStepJournal"), "{shown}");
    contract(&journal).await;
    // Two replicas writing the same revision: exactly one wins.
    let racing = format!("race-{}", uuid::Uuid::new_v4());
    let (first, second) = (entry(1), entry(1));
    let (a, b) = tokio::join!(
        journal.save(&racing, &first),
        journal.save(&racing, &second)
    );
    assert_eq!(
        [a.is_ok(), b.is_ok()].iter().filter(|ok| **ok).count(),
        1,
        "{a:?} {b:?}"
    );
    let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await;
}
