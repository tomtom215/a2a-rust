// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The journal contract, held by every store: a write is accepted only as the
//! successor of the revision stored, a checkpoint comes back as it went in, and
//! keys are independent.

use std::collections::{BTreeMap, HashMap};

use crate::tool_loop::journal::{
    ActionReceipt, CHECKPOINT_VERSION, InMemoryStepJournal, JournalError, PendingApproval, Pins,
    ReceiptState, RunCheckpoint, StepJournal,
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
    }
}

async fn contract(journal: &dyn StepJournal) {
    let key = format!("run-{}", uuid::Uuid::new_v4());
    assert_eq!(journal.load(&key).await, Ok(None));

    // The first write is revision 1, and only that.
    assert_eq!(
        journal.save(&key, &checkpoint(2)).await,
        Err(JournalError::Conflict),
        "a first write that skips revision 1"
    );
    assert_eq!(journal.save(&key, &checkpoint(1)).await, Ok(()));
    assert_eq!(
        journal.save(&key, &checkpoint(1)).await,
        Err(JournalError::Conflict),
        "the same revision twice"
    );
    assert_eq!(
        journal.save(&key, &checkpoint(3)).await,
        Err(JournalError::Conflict),
        "a revision skipped"
    );
    assert_eq!(journal.save(&key, &checkpoint(2)).await, Ok(()));
    assert_eq!(
        journal.load(&key).await,
        Ok(Some(checkpoint(2))),
        "a checkpoint comes back as it went in"
    );

    // Keys are independent.
    let other = format!("run-{}", uuid::Uuid::new_v4());
    assert_eq!(journal.save(&other, &checkpoint(1)).await, Ok(()));
    assert_eq!(journal.load(&key).await, Ok(Some(checkpoint(2))));

    journal.clear(&key).await.unwrap();
    assert_eq!(journal.load(&key).await, Ok(None));
    assert_eq!(journal.load(&other).await, Ok(Some(checkpoint(1))));
    // A cleared key starts again at revision 1.
    assert_eq!(journal.save(&key, &checkpoint(1)).await, Ok(()));
    // Clearing what is not there is not an error.
    journal.clear("never-written").await.unwrap();
}

#[tokio::test]
async fn the_in_memory_journal_keeps_the_contract() {
    let journal = InMemoryStepJournal::new();
    assert!(journal.is_empty());
    contract(&journal).await;
    assert_eq!(journal.len(), 2);
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
    first.save("run", &checkpoint(1)).await.unwrap();
    drop(first);
    let second = crate::tool_loop::SqliteStepJournal::new(&url)
        .await
        .unwrap();
    assert_eq!(second.load("run").await, Ok(Some(checkpoint(1))));
    assert_eq!(second.save("run", &checkpoint(2)).await, Ok(()));
    let _ = std::fs::remove_file(&path);
}

/// An unreadable row is an error, not an absent run: resuming from nothing
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
    assert!(matches!(
        journal.load("r").await,
        Err(JournalError::Store(_))
    ));
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
    let (first, second) = (checkpoint(1), checkpoint(1));
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
