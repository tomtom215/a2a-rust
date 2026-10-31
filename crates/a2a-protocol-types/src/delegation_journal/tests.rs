// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The journal contract, one clause at a time.
//!
//! The in-memory journal's futures are ready on first poll, so a no-op waker
//! and one poll drive them; this crate has no async runtime to borrow.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use super::*;

fn now<T>(future: impl Future<Output = A2aResult<T>>) -> T {
    let mut cx = Context::from_waker(Waker::noop());
    match pin!(future).poll(&mut cx) {
        Poll::Ready(done) => done.expect("the in-memory journal does not fail"),
        Poll::Pending => panic!("the in-memory journal answers at once"),
    }
}

fn record(id: &str, owner: &str, at: i64) -> DelegationRecord {
    DelegationRecord::new(id, owner, "http://worker:8080", at)
}

#[test]
fn begin_is_idempotent_by_id_and_keeps_the_first_record() {
    let journal = InMemoryDelegationJournal::new();
    now(journal.begin(&record("r1", "sup", 10).with_idempotency_key(Some("k1".into()))));
    now(journal.begin(&record("r1", "other", 99).with_idempotency_key(Some("k2".into()))));
    let kept = journal.get("r1").expect("recorded");
    assert_eq!(kept.owner, "sup");
    assert_eq!(kept.idempotency_key.as_deref(), Some("k1"));
    assert_eq!(kept.started_at_ms, 10);
    assert_eq!(now(journal.open("sup")).len(), 1);
    assert_eq!(now(journal.open("other")).len(), 0);
}

#[test]
fn progress_sets_what_is_given_and_never_clears_what_is_not() {
    let journal = InMemoryDelegationJournal::new();
    now(journal.begin(&record("r1", "sup", 10)));
    now(journal.progress("r1", Some("task-1"), None, 20));
    let r = journal.get("r1").unwrap();
    assert_eq!(r.task_id.as_deref(), Some("task-1"));
    assert_eq!(r.last_event_id, None);
    assert_eq!(r.updated_at_ms, 20);

    now(journal.progress("r1", None, Some("evt-7"), 30));
    let r = journal.get("r1").unwrap();
    assert_eq!(
        r.task_id.as_deref(),
        Some("task-1"),
        "a position alone keeps the id"
    );
    assert_eq!(r.last_event_id.as_deref(), Some("evt-7"));
    assert_eq!(r.updated_at_ms, 30);
    assert_eq!(r.started_at_ms, 10, "the start never moves");
}

#[test]
fn progress_on_an_unknown_record_is_not_an_error() {
    let journal = InMemoryDelegationJournal::new();
    now(journal.progress("purged", Some("t"), Some("e"), 5));
    now(journal.settle("purged", "completed", 6));
    assert_eq!(journal.get("purged"), None, "and does not create one");
}

#[test]
fn a_record_settles_once_and_a_settled_record_stops_changing() {
    let journal = InMemoryDelegationJournal::new();
    now(journal.begin(&record("r1", "sup", 10)));
    now(journal.settle("r1", "completed", 40));
    now(journal.settle("r1", "lost", 50));
    now(journal.progress("r1", Some("late"), Some("late"), 60));
    let r = journal.get("r1").unwrap();
    assert_eq!(r.outcome.as_deref(), Some("completed"), "never rewritten");
    assert_eq!(r.task_id, None);
    assert_eq!(r.last_event_id, None);
    assert_eq!(r.updated_at_ms, 40);
    assert!(!r.is_open());
}

#[test]
fn open_lists_one_owners_unsettled_records_oldest_first() {
    let journal = InMemoryDelegationJournal::new();
    now(journal.begin(&record("c", "sup", 30)));
    now(journal.begin(&record("a", "sup", 10)));
    now(journal.begin(&record("b", "sup", 10)));
    now(journal.begin(&record("done", "sup", 5)));
    now(journal.begin(&record("theirs", "someone", 1)));
    now(journal.settle("done", "completed", 6));
    let ids: Vec<_> = now(journal.open("sup")).into_iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        ["a", "b", "c"],
        "by start, ties by id; no settled, no other owner"
    );
}

#[test]
fn purge_deletes_only_what_settled_before_the_cutoff() {
    let journal = InMemoryDelegationJournal::new();
    for (id, at) in [("old", 10), ("edge", 20), ("new", 30), ("open", 1)] {
        now(journal.begin(&record(id, "sup", at)));
    }
    now(journal.settle("old", "completed", 15));
    now(journal.settle("edge", "failed", 20));
    now(journal.settle("new", "canceled", 25));
    // Settled at 15 goes; settled at exactly the cutoff stays; open stays.
    assert_eq!(now(journal.purge_settled(20)), 1);
    assert_eq!(journal.get("old"), None);
    assert!(journal.get("edge").is_some());
    assert!(journal.get("new").is_some());
    assert!(journal.get("open").is_some_and(|r| r.is_open()));
    assert_eq!(now(journal.purge_settled(20)), 0, "nothing more to remove");
    assert_eq!(now(journal.purge_settled(1_000)), 2);
    assert!(
        journal.get("open").is_some(),
        "an open record is never purged"
    );
}

#[test]
fn a_record_round_trips_in_camel_case_and_leaves_out_what_it_lacks() {
    let bare = record("r1", "sup", 10);
    let json = serde_json::to_value(&bare).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "id": "r1", "owner": "sup", "agent": "http://worker:8080",
            "startedAtMs": 10, "updatedAtMs": 10
        })
    );
    let mut full = bare
        .with_tenant(Some("acme".into()))
        .with_idempotency_key(Some("k".into()));
    full.task_id = Some("t".into());
    full.last_event_id = Some("e".into());
    full.outcome = Some("completed".into());
    let back: DelegationRecord =
        serde_json::from_value(serde_json::to_value(&full).unwrap()).unwrap();
    assert_eq!(back, full);
    let json = serde_json::to_value(&full).unwrap();
    for key in [
        "tenant",
        "idempotencyKey",
        "taskId",
        "lastEventId",
        "outcome",
    ] {
        assert!(json.get(key).is_some(), "{key}");
    }
}
