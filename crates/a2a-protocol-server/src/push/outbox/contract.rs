// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The rules every [`PushOutbox`] keeps, as one suite each store runs: the
//! in-memory one in `tests.rs`, SQLite in `sqlite_tests.rs`, PostgreSQL in
//! `postgres_tests.rs` (against a live database).
//!
//! No test here waits for a clock: a lease or a backoff of zero has run out
//! the moment it is taken, by every store's clock, and one of an hour has
//! not.

use std::time::Duration;

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use super::{NewDelivery, PushOutbox};

const HOUR: Duration = Duration::from_secs(3600);

fn event(state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t1"),
        ContextId::new("c1"),
        TaskStatus::new(state),
    ))
}

/// A delivery of an event in `state` to `config` of task `t1` in `tenant`.
pub fn delivery(tenant: &str, config: &str, state: TaskState) -> NewDelivery {
    let event = event(state);
    NewDelivery {
        delivery_id: uuid::Uuid::new_v4().to_string(),
        tenant: tenant.to_owned(),
        caller: None,
        task_id: "t1".to_owned(),
        config_id: config.to_owned(),
        ends_turn: crate::handler::ends_turn(&event),
        event,
    }
}

/// An event as JSON: `StreamResponse` has no `PartialEq`.
fn json(event: &StreamResponse) -> serde_json::Value {
    serde_json::to_value(event).unwrap()
}

fn ids(claimed: &[super::ClaimedDelivery]) -> Vec<String> {
    claimed
        .iter()
        .map(|c| c.delivery.delivery_id.clone())
        .collect()
}

/// Recorded rows come back as they went in; a webhook's later row waits for
/// its earlier one, while another webhook's first row does not.
pub async fn claims_the_first_row_of_each_webhook_in_order(outbox: &dyn PushOutbox) {
    let rows = vec![
        delivery("", "a", TaskState::Working),
        delivery("", "a", TaskState::Completed),
        delivery("", "b", TaskState::Working),
    ];
    let (a1, a2, b1) = (
        rows[0].delivery_id.clone(),
        rows[1].delivery_id.clone(),
        rows[2].delivery_id.clone(),
    );
    assert_eq!(outbox.record(rows, 10).await.unwrap(), 0);
    assert_eq!(outbox.pending().await.unwrap(), 3);

    let first = outbox.claim("me", 10, HOUR).await.unwrap();
    assert_eq!(ids(&first), [a1.clone(), b1.clone()]);
    assert_eq!(
        json(&first[0].delivery.event),
        json(&event(TaskState::Working))
    );
    assert_eq!(first[0].attempts, 0);
    assert!(first[0].age < HOUR);
    assert!(
        outbox.claim("other", 10, HOUR).await.unwrap().is_empty(),
        "leased, and the later row of `a` waits for the first"
    );

    outbox.complete(first[0].seq).await.unwrap();
    let second = outbox.claim("other", 10, HOUR).await.unwrap();
    assert_eq!(ids(&second), [a2]);
    assert!(second[0].delivery.ends_turn);
    assert_eq!(outbox.pending().await.unwrap(), 2);
}

/// At most `max` rows per claim.
pub async fn a_claim_takes_at_most_max(outbox: &dyn PushOutbox) {
    let rows: Vec<NewDelivery> = ["a", "b", "c"]
        .iter()
        .map(|c| delivery("", c, TaskState::Working))
        .collect();
    outbox.record(rows, 10).await.unwrap();
    assert_eq!(outbox.claim("me", 2, HOUR).await.unwrap().len(), 2);
    assert_eq!(outbox.claim("me", 2, HOUR).await.unwrap().len(), 1);
}

/// A lease that has run out (zero here) leaves the row to the next claim:
/// the owner stopped.
pub async fn a_lapsed_lease_is_claimed_again(outbox: &dyn PushOutbox) {
    let row = delivery("", "a", TaskState::Working);
    let id = row.delivery_id.clone();
    outbox.record(vec![row], 10).await.unwrap();
    assert_eq!(
        ids(&outbox.claim("gone", 10, Duration::ZERO).await.unwrap()),
        std::slice::from_ref(&id)
    );
    let again = outbox.claim("next", 10, HOUR).await.unwrap();
    assert_eq!(ids(&again), [id], "the same row, the same delivery id");
}

/// `retry_after` releases the row one attempt on, due after the backoff; by
/// an owner that does not hold the lease it changes nothing.
pub async fn retry_after_releases_the_row_to_its_owner_only(outbox: &dyn PushOutbox) {
    outbox
        .record(vec![delivery("", "a", TaskState::Working)], 10)
        .await
        .unwrap();
    let seq = outbox.claim("me", 10, HOUR).await.unwrap()[0].seq;

    outbox
        .retry_after(seq, "not-me", Duration::ZERO)
        .await
        .unwrap();
    assert!(
        outbox.claim("not-me", 10, HOUR).await.unwrap().is_empty(),
        "still leased to its owner"
    );

    outbox.retry_after(seq, "me", HOUR).await.unwrap();
    assert!(
        outbox.claim("me", 10, HOUR).await.unwrap().is_empty(),
        "released, but not due for an hour"
    );
}

/// The attempt count goes up by one per release, and the row comes back.
pub async fn each_release_counts_an_attempt(outbox: &dyn PushOutbox) {
    outbox
        .record(vec![delivery("", "a", TaskState::Working)], 10)
        .await
        .unwrap();
    for attempts in 0..3 {
        let claimed = outbox.claim("me", 10, HOUR).await.unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].attempts, attempts);
        outbox
            .retry_after(claimed[0].seq, "me", Duration::ZERO)
            .await
            .unwrap();
    }
}

/// Past the bound, recording drops the oldest row that is neither claimed
/// nor ends a turn; with none, the new row unless it ends a turn, which is
/// kept past the bound.
pub async fn the_bound_drops_what_the_lanes_drop(outbox: &dyn PushOutbox) {
    let w1 = delivery("", "a", TaskState::Working);
    let w2 = delivery("", "a", TaskState::Working);
    let w1_id = w1.delivery_id.clone();
    outbox.record(vec![w1, w2], 2).await.unwrap();
    // The first is claimed, so the oldest unclaimed is `w2`.
    let claimed = outbox.claim("me", 10, HOUR).await.unwrap();
    assert_eq!(ids(&claimed), [w1_id]);
    let w3 = delivery("", "a", TaskState::Working);
    assert_eq!(outbox.record(vec![w3], 2).await.unwrap(), 1, "w2 dropped");
    assert_eq!(outbox.pending().await.unwrap(), 2);

    // Now `w1` (claimed) and `w3`: drop `w3` for an ending row.
    let end1 = delivery("", "a", TaskState::Completed);
    assert_eq!(outbox.record(vec![end1], 2).await.unwrap(), 1);
    assert_eq!(outbox.pending().await.unwrap(), 2);
    // `w1` (claimed) and `end1`: nothing can go, so a working row is not kept
    // and an ending one is, past the bound.
    let w4 = delivery("", "a", TaskState::Working);
    assert_eq!(outbox.record(vec![w4], 2).await.unwrap(), 1, "w4 not kept");
    let end2 = delivery("", "a", TaskState::Failed);
    assert_eq!(outbox.record(vec![end2], 2).await.unwrap(), 0);
    assert_eq!(outbox.pending().await.unwrap(), 3);
}

/// Tenants (and callers) are separate webhooks, even with one task and
/// config id.
pub async fn tenants_and_callers_are_separate_webhooks(outbox: &dyn PushOutbox) {
    let mut isolated = delivery("t", "a", TaskState::Working);
    isolated.caller = Some("alice".to_owned());
    let rows = vec![
        delivery("t", "a", TaskState::Working),
        delivery("u", "a", TaskState::Working),
        isolated.clone(),
    ];
    outbox.record(rows, 10).await.unwrap();
    let claimed = outbox.claim("me", 10, HOUR).await.unwrap();
    assert_eq!(claimed.len(), 3, "three webhooks, one row each");
    assert_eq!(claimed[2].delivery.caller.as_deref(), Some("alice"));
    assert_eq!(claimed[2].delivery.tenant, "t");
}

/// `complete` of a row that is gone is harmless.
pub async fn completing_twice_is_harmless(outbox: &dyn PushOutbox) {
    outbox
        .record(vec![delivery("", "a", TaskState::Working)], 10)
        .await
        .unwrap();
    let seq = outbox.claim("me", 10, HOUR).await.unwrap()[0].seq;
    outbox.complete(seq).await.unwrap();
    outbox.complete(seq).await.unwrap();
    assert_eq!(outbox.pending().await.unwrap(), 0);
}

/// Every rule, on a fresh outbox each: `fresh` makes one.
pub async fn run_all<F, Fut, O>(fresh: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = O>,
    O: PushOutbox,
{
    claims_the_first_row_of_each_webhook_in_order(&fresh().await).await;
    a_claim_takes_at_most_max(&fresh().await).await;
    a_lapsed_lease_is_claimed_again(&fresh().await).await;
    retry_after_releases_the_row_to_its_owner_only(&fresh().await).await;
    each_release_counts_an_attempt(&fresh().await).await;
    the_bound_drops_what_the_lanes_drop(&fresh().await).await;
    tenants_and_callers_are_separate_webhooks(&fresh().await).await;
    completing_twice_is_harmless(&fresh().await).await;
}
