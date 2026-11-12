// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The reservation half of the outbox contract (ADR 0037, audit H14): reserving,
//! arming, releasing, what is held, delivery ids as identity, and renewal. The
//! helpers are `contract`'s.

use super::*;

/// A reserved row is not claimable until it is armed, and arming it makes it
/// claimable like any other.
pub(super) async fn a_reserved_row_is_claimed_only_once_armed(outbox: &dyn PushOutbox) {
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
pub(super) async fn an_unarmed_row_holds_its_webhooks_place(outbox: &dyn PushOutbox) {
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
pub(super) async fn release_removes_unarmed_rows_only(outbox: &dyn PushOutbox) {
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
pub(super) async fn held_lists_lapsed_unarmed_reservations_once(outbox: &dyn PushOutbox) {
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
pub(super) async fn held_is_oldest_first_and_limited(outbox: &dyn PushOutbox) {
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
pub(super) async fn the_bound_never_drops_a_reserved_row(outbox: &dyn PushOutbox) {
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
pub(super) async fn a_delivery_id_is_recorded_once(outbox: &dyn PushOutbox) {
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
    assert_eq!(ids(&claimed), std::slice::from_ref(&id));
    outbox.complete(claimed[0].seq).await.unwrap();
    outbox.record(vec![row], 10).await.unwrap();
    assert_eq!(ids(&outbox.claim("me", 10, HOUR).await.unwrap()), [id]);
}

/// `held_for_task` lists the unarmed reservations of one task, in one tenant
/// and caller, held or not.
pub(super) async fn held_for_task_lists_that_tasks_unarmed_reservations(outbox: &dyn PushOutbox) {
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
pub(super) async fn renew_applies_to_the_owners_live_leases_only(outbox: &dyn PushOutbox) {
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

/// A duplicate of a row that is already there, recorded into a webhook that is
/// at its bound, drops nothing: it is not a new row, so it needs no room.
pub(super) async fn recording_a_duplicate_drops_no_other_row(outbox: &dyn PushOutbox) {
    let (first, second) = (
        delivery("", "a", TaskState::Working),
        delivery("", "a", TaskState::Working),
    );
    let (first_id, second_id) = (first.delivery_id.clone(), second.delivery_id.clone());
    outbox.record(vec![first, second.clone()], 2).await.unwrap();
    assert_eq!(
        outbox.record(vec![second], 2).await.unwrap(),
        0,
        "nothing dropped for a row that is already recorded"
    );
    assert_eq!(outbox.pending().await.unwrap(), 2);
    let one = outbox.claim("me", 10, HOUR).await.unwrap();
    assert_eq!(ids(&one), [first_id]);
    outbox.complete(one[0].seq).await.unwrap();
    assert_eq!(
        ids(&outbox.claim("me", 10, HOUR).await.unwrap()),
        [second_id]
    );
}

/// Extending a hold moves a lapsed, unarmed reservation out of `held` and back
/// in once the new hold has run out; an armed or unknown reservation has
/// nothing to extend (adversarial review R3: a live writer keeps its
/// reservation, a stopped one does not).
pub(super) async fn extend_hold_keeps_a_live_reservation_out_of_held(outbox: &dyn PushOutbox) {
    outbox
        .reserve(
            "live",
            1,
            vec![
                delivery("", "a", TaskState::Working),
                delivery("", "b", TaskState::Working),
            ],
            10,
            NOW,
        )
        .await
        .unwrap();
    outbox
        .reserve(
            "dead",
            2,
            vec![delivery("", "c", TaskState::Working)],
            10,
            NOW,
        )
        .await
        .unwrap();
    let names = |held: Vec<crate::push::outbox::Held>| {
        let mut names: Vec<String> = held.into_iter().map(|h| h.reservation).collect();
        names.sort();
        names
    };
    assert_eq!(names(outbox.held(10).await.unwrap()), ["dead", "live"]);

    assert_eq!(
        outbox.extend_hold("live", HOUR).await.unwrap(),
        2,
        "both of its rows"
    );
    assert_eq!(
        names(outbox.held(10).await.unwrap()),
        ["dead"],
        "a reservation whose hold was extended is not settled"
    );

    outbox.arm("dead").await.unwrap();
    assert_eq!(
        outbox.extend_hold("dead", HOUR).await.unwrap(),
        0,
        "an armed reservation has no hold"
    );
    assert_eq!(
        outbox.extend_hold("never-existed", HOUR).await.unwrap(),
        0,
        "nor has one that is not there"
    );
    outbox.release("live").await.unwrap();
    assert_eq!(outbox.extend_hold("live", HOUR).await.unwrap(), 0);
}
