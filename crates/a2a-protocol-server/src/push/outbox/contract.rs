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

const NOW: Duration = Duration::ZERO;

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

/// A reserved row is not claimable until it is armed, and arming it makes it
/// claimable like any other.
pub async fn a_reserved_row_is_claimed_only_once_armed(outbox: &dyn PushOutbox) {
    assert!(outbox.supports_reservations());
    let row = delivery("", "a", TaskState::Working);
    let id = row.delivery_id.clone();
    assert_eq!(
        outbox.reserve("r1", 7, vec![row], 10, HOUR).await.unwrap(),
        0
    );
    assert_eq!(
        outbox.pending().await.unwrap(),
        1,
        "reserved rows are pending"
    );
    assert!(
        outbox.claim("me", 10, HOUR).await.unwrap().is_empty(),
        "not armed, so not claimed"
    );
    assert_eq!(outbox.arm("r1").await.unwrap(), 1);
    assert_eq!(ids(&outbox.claim("me", 10, HOUR).await.unwrap()), [id]);
    assert_eq!(
        outbox.arm("r1").await.unwrap(),
        1,
        "arming again finds the row, armed"
    );
}

/// An unarmed row holds its webhook's place: a later row of the same webhook
/// is not claimed past it, another webhook's is.
pub async fn an_unarmed_row_holds_its_webhooks_place(outbox: &dyn PushOutbox) {
    let reserved = delivery("", "a", TaskState::Working);
    let (later, other) = (
        delivery("", "a", TaskState::Completed),
        delivery("", "b", TaskState::Working),
    );
    let (reserved_id, _later_id, other_id) = (
        reserved.delivery_id.clone(),
        later.delivery_id.clone(),
        other.delivery_id.clone(),
    );
    outbox
        .reserve("r1", 1, vec![reserved], 10, HOUR)
        .await
        .unwrap();
    outbox.record(vec![later, other], 10).await.unwrap();
    assert_eq!(
        ids(&outbox.claim("me", 10, HOUR).await.unwrap()),
        [other_id],
        "`a`'s later row waits for the reserved one"
    );
    outbox.arm("r1").await.unwrap();
    assert_eq!(
        ids(&outbox.claim("me", 10, HOUR).await.unwrap()),
        [reserved_id]
    );
    assert!(outbox.claim("me", 10, HOUR).await.unwrap().is_empty());
}

/// Releasing removes the unarmed rows of a reservation, and only those.
pub async fn release_removes_unarmed_rows_only(outbox: &dyn PushOutbox) {
    outbox
        .reserve(
            "gone",
            1,
            vec![
                delivery("", "a", TaskState::Working),
                delivery("", "b", TaskState::Working),
            ],
            10,
            HOUR,
        )
        .await
        .unwrap();
    outbox
        .reserve(
            "kept",
            2,
            vec![delivery("", "c", TaskState::Working)],
            10,
            HOUR,
        )
        .await
        .unwrap();
    outbox.arm("kept").await.unwrap();
    outbox.release("gone").await.unwrap();
    outbox.release("kept").await.unwrap();
    outbox.release("never-existed").await.unwrap();
    assert_eq!(
        outbox.pending().await.unwrap(),
        1,
        "only the armed row is left"
    );
    assert_eq!(outbox.claim("me", 10, HOUR).await.unwrap().len(), 1);
    assert_eq!(
        outbox.arm("gone").await.unwrap(),
        0,
        "released: nothing to arm"
    );
}

/// `held` lists a reservation once, however many rows it has, only when it is
/// not armed and its hold has run out; it names where the event belongs.
pub async fn held_lists_lapsed_unarmed_reservations_once(outbox: &dyn PushOutbox) {
    let mut isolated = delivery("t", "a", TaskState::Completed);
    isolated.caller = Some("alice".to_owned());
    let mut second = isolated.clone();
    second.config_id = "b".to_owned();
    second.delivery_id = uuid::Uuid::new_v4().to_string();
    outbox
        .reserve("lapsed", 42, vec![isolated, second], 10, NOW)
        .await
        .unwrap();
    outbox
        .reserve(
            "fresh",
            43,
            vec![delivery("", "a", TaskState::Working)],
            10,
            HOUR,
        )
        .await
        .unwrap();
    outbox
        .reserve(
            "armed",
            44,
            vec![delivery("", "z", TaskState::Working)],
            10,
            NOW,
        )
        .await
        .unwrap();
    outbox.arm("armed").await.unwrap();

    let held = outbox.held(10).await.unwrap();
    assert_eq!(held.len(), 1, "{held:?}");
    let h = &held[0];
    assert_eq!(h.reservation, "lapsed");
    assert_eq!(
        (h.tenant.as_str(), h.caller.as_deref()),
        ("t", Some("alice"))
    );
    assert_eq!((h.task_id.as_str(), h.log_seq), ("t1", 42));
    assert_eq!(json(&h.event), json(&event(TaskState::Completed)));
    assert_eq!(outbox.held(0).await.unwrap().len(), 0, "limit 0");
}

/// `held` is oldest first and takes at most `limit`.
pub async fn held_is_oldest_first_and_limited(outbox: &dyn PushOutbox) {
    for (n, config) in ["a", "b", "c"].into_iter().enumerate() {
        outbox
            .reserve(
                &format!("r{n}"),
                n as u64 + 1,
                vec![delivery("", config, TaskState::Working)],
                10,
                NOW,
            )
            .await
            .unwrap();
    }
    let held = outbox.held(2).await.unwrap();
    let names: Vec<&str> = held.iter().map(|h| h.reservation.as_str()).collect();
    assert_eq!(names, ["r0", "r1"]);
}

/// An unarmed row is never the one dropped to make room: with nothing else to
/// drop, a new row that does not end a turn is the one dropped.
pub async fn the_bound_never_drops_a_reserved_row(outbox: &dyn PushOutbox) {
    outbox
        .reserve(
            "r1",
            1,
            vec![delivery("", "a", TaskState::Working)],
            1,
            HOUR,
        )
        .await
        .unwrap();
    let dropped = outbox
        .record(vec![delivery("", "a", TaskState::Working)], 1)
        .await
        .unwrap();
    assert_eq!(dropped, 1, "the new row, not the reserved one");
    assert_eq!(outbox.pending().await.unwrap(), 1);
    assert_eq!(
        outbox.arm("r1").await.unwrap(),
        1,
        "the reserved row is intact"
    );
}

/// Recording a delivery id twice keeps one row; once the row is delivered and
/// gone, recording it again brings it back under the same id.
pub async fn a_delivery_id_is_recorded_once(outbox: &dyn PushOutbox) {
    let row = delivery("", "a", TaskState::Working);
    let id = row.delivery_id.clone();
    assert_eq!(outbox.record(vec![row.clone()], 10).await.unwrap(), 0);
    assert_eq!(outbox.record(vec![row.clone()], 10).await.unwrap(), 0);
    assert_eq!(outbox.pending().await.unwrap(), 1, "one row, not two");
    assert_eq!(
        outbox
            .reserve("r", 3, vec![row.clone()], 10, HOUR)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        outbox.pending().await.unwrap(),
        1,
        "a reservation does not duplicate it"
    );

    let claimed = outbox.claim("me", 10, HOUR).await.unwrap();
    assert_eq!(ids(&claimed), [id.clone()]);
    outbox.complete(claimed[0].seq).await.unwrap();
    outbox.record(vec![row], 10).await.unwrap();
    assert_eq!(ids(&outbox.claim("me", 10, HOUR).await.unwrap()), [id]);
}

/// `held_for_task` lists the unarmed reservations of one task, in one tenant
/// and caller, held or not.
pub async fn held_for_task_lists_that_tasks_unarmed_reservations(outbox: &dyn PushOutbox) {
    let mut other_task = delivery("", "a", TaskState::Working);
    other_task.task_id = "t2".to_owned();
    let mut other_caller = delivery("", "a", TaskState::Working);
    other_caller.caller = Some("bob".to_owned());
    outbox
        .reserve(
            "mine",
            5,
            vec![delivery("", "a", TaskState::Working)],
            10,
            HOUR,
        )
        .await
        .unwrap();
    outbox
        .reserve("t2", 6, vec![other_task], 10, HOUR)
        .await
        .unwrap();
    outbox
        .reserve("bob", 7, vec![other_caller], 10, HOUR)
        .await
        .unwrap();
    outbox
        .reserve(
            "armed",
            8,
            vec![delivery("", "b", TaskState::Working)],
            10,
            HOUR,
        )
        .await
        .unwrap();
    outbox.arm("armed").await.unwrap();

    let held = outbox.held_for_task("", None, "t1").await.unwrap();
    let names: Vec<&str> = held.iter().map(|h| h.reservation.as_str()).collect();
    assert_eq!(
        names,
        ["mine"],
        "its hold has not run out, and it is listed"
    );
    assert_eq!(held[0].log_seq, 5);
    assert!(
        outbox
            .held_for_task("other", None, "t1")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        outbox
            .held_for_task("", Some("bob"), "t1")
            .await
            .unwrap()
            .len(),
        1
    );
}

/// `renew` gives its owner's live leases the lease asked for, and touches no
/// one else's, and none that ran out.
pub async fn renew_applies_to_the_owners_live_leases_only(outbox: &dyn PushOutbox) {
    assert!(outbox.supports_renewal());
    outbox
        .record(
            vec![
                delivery("", "a", TaskState::Working),
                delivery("", "b", TaskState::Working),
            ],
            10,
        )
        .await
        .unwrap();
    let claimed = outbox.claim("me", 1, HOUR).await.unwrap();
    assert_eq!(claimed.len(), 1);
    let other = outbox.claim("them", 1, Duration::ZERO).await.unwrap();
    assert_eq!(other.len(), 1);

    assert_eq!(outbox.renew("not-me", HOUR).await.unwrap(), 0);
    assert_eq!(
        outbox.renew("them", HOUR).await.unwrap(),
        0,
        "a lease that ran out is not renewed"
    );
    assert_eq!(outbox.renew("me", HOUR).await.unwrap(), 1);
    // The lease asked for is the one the row has: a lease of nothing ends it.
    assert_eq!(outbox.renew("me", Duration::ZERO).await.unwrap(), 1);
    assert_eq!(
        ids(&outbox.claim("next", 10, HOUR).await.unwrap()).len(),
        2,
        "both rows are free again"
    );
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
    a_reserved_row_is_claimed_only_once_armed(&fresh().await).await;
    an_unarmed_row_holds_its_webhooks_place(&fresh().await).await;
    release_removes_unarmed_rows_only(&fresh().await).await;
    held_lists_lapsed_unarmed_reservations_once(&fresh().await).await;
    held_is_oldest_first_and_limited(&fresh().await).await;
    the_bound_never_drops_a_reserved_row(&fresh().await).await;
    a_delivery_id_is_recorded_once(&fresh().await).await;
    held_for_task_lists_that_tasks_unarmed_reservations(&fresh().await).await;
    renew_applies_to_the_owners_live_leases_only(&fresh().await).await;
}
