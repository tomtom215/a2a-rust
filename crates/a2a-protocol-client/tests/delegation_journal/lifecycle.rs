// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The record is written first, settled however the delegation ends, and found
//! again by a restarted supervisor.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use a2a_protocol_client::delegation::{Delegation, Outcome, ResumePolicy};
use a2a_protocol_types::delegation_journal::DelegationJournal;

use super::harness::*;

#[tokio::test]
async fn the_record_is_written_before_the_child_exists_and_settled_with_its_outcome() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    let key = "k".repeat(32);
    let text = format!("done:{}", uuid::Uuid::new_v4());
    let before = unix_ms();
    let d = start(&client, keyed(&text, &key), &journal).await;
    let record_id = d.journal_id().expect("journaled").to_owned();
    // A delegation prints its record's id and nothing of the journal itself.
    let shown = format!("{d:?}");
    assert!(
        shown.contains(&record_id) && shown.contains("Journaled"),
        "{shown}"
    );
    let done = d.wait(std::future::pending()).await;
    let after = unix_ms();
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");

    assert!(
        journal.began_at.load(Ordering::SeqCst) < STARTS.lock().unwrap()[&text],
        "begin ran before the executor started"
    );
    let r = journal.inner.get(&record_id).expect("recorded");
    assert_eq!(r.owner, "supervisor-1");
    assert_eq!(r.agent, "http://worker.example");
    assert_eq!(r.idempotency_key.as_deref(), Some(key.as_str()));
    assert_eq!(r.task_id, done.task_id, "the child's id");
    assert_eq!(r.outcome.as_deref(), Some("completed"));
    // The times are the wall clock's, in Unix milliseconds: written at the start
    // and rewritten when the delegation settled.
    assert!(
        (before..=after).contains(&r.started_at_ms),
        "started {} outside {before}..={after}",
        r.started_at_ms
    );
    assert!(
        r.started_at_ms <= r.updated_at_ms && r.updated_at_ms <= after,
        "updated {} after started {} and before {after}",
        r.updated_at_ms,
        r.started_at_ms
    );
}

#[tokio::test]
async fn a_journal_that_cannot_record_starts_nothing() {
    let client = client_for(serve().await);
    let journal = Probe::new(true);
    let text = format!("done:{}", uuid::Uuid::new_v4());
    let err = Delegation::start_journaled(
        Arc::clone(&client),
        job(&text),
        journal as Arc<dyn DelegationJournal>,
        "supervisor-1",
        "http://worker.example",
    )
    .await
    .expect_err("refused");
    assert!(err.to_string().contains("the journal is full"), "{err}");
    // No message was sent, so no executor started.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!STARTS.lock().unwrap().contains_key(&text));
}

#[tokio::test]
async fn a_restarted_supervisor_finds_its_open_child_and_picks_it_up() {
    let upstream = serve().await;
    let journal = Probe::new(false);
    let hold = key();

    // The first supervisor starts the child, then goes away without ending it.
    let mut first = start(&client_for(upstream), job(&hold), &journal).await;
    let id = until_named(&mut first).await;
    let record_id = first.journal_id().unwrap().to_owned();
    assert_eq!(first.detach().as_deref(), Some(id.as_str()));

    // The second finds it in the journal: open, named, and not settled.
    let open = journal.open("supervisor-1").await.expect("open");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, record_id);
    assert_eq!(open[0].task_id.as_deref(), Some(id.as_str()));
    assert!(journal.open("someone-else").await.expect("open").is_empty());

    let again = Delegation::reattach(
        client_for(upstream),
        open[0].tenant.clone(),
        id.clone(),
        open[0].last_event_id.clone(),
    )
    .await
    .expect("reattached")
    .with_journal(
        Arc::clone(&journal) as Arc<dyn DelegationJournal>,
        &open[0].id,
    );
    gate(&hold).notify_one();
    let done = tokio::time::timeout(GUARD, again.wait(std::future::pending()))
        .await
        .expect("bounded");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
    assert_eq!(
        journal.inner.get(&record_id).unwrap().outcome.as_deref(),
        Some("completed"),
        "the record the first supervisor began is settled by the second"
    );
    assert!(journal.open("supervisor-1").await.expect("open").is_empty());
}

#[tokio::test]
async fn every_way_a_delegation_ends_settles_its_record() {
    let upstream = serve().await;
    let client = client_for(upstream);

    // Lost, with resuming off: the child abandons its task, the stream ends.
    let journal = Probe::new(false);
    let d = start(&client, job("abandon"), &journal)
        .await
        .with_resume(ResumePolicy::none());
    let record = d.journal_id().unwrap().to_owned();
    let done = d.wait(std::future::pending()).await;
    assert!(matches!(done.outcome, Outcome::Lost(_)));
    assert_eq!(
        journal.inner.get(&record).unwrap().outcome.as_deref(),
        Some("lost")
    );

    // Cancelled by the parent.
    let journal = Probe::new(false);
    let hold = key();
    let mut d = start(&client, job(&hold), &journal).await;
    until_named(&mut d).await;
    let record = d.journal_id().unwrap().to_owned();
    let done = d.cancel().await;
    let label = journal
        .inner
        .get(&record)
        .unwrap()
        .outcome
        .expect("settled");
    assert!(
        matches!(label.as_str(), "canceled" | "cancel_requested"),
        "{label} for {:?}",
        done.outcome
    );

    // Dropped unsettled: the drop-time cancel settles it too.
    let journal = Probe::new(false);
    let hold = key();
    let mut d = start(&client, job(&hold), &journal).await;
    until_named(&mut d).await;
    let record = d.journal_id().unwrap().to_owned();
    drop(d);
    tokio::time::timeout(GUARD, async {
        while journal.inner.get(&record).unwrap().is_open() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the dropped delegation's record was settled");
}

#[test]
fn a_delegation_without_a_journal_has_no_record() {
    // `Delegation::start` is unchanged: no journal, no id.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let client = client_for(serve().await);
        let d = Delegation::start(client, job("done")).await.expect("start");
        assert_eq!(d.journal_id(), None);
        let _ = d.wait(std::future::pending()).await;
    });
}
