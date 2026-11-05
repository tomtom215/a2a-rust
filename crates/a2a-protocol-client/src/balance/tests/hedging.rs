// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Hedging against the in-memory endpoints of `balance::tests`, under a
//! paused clock: when a copy is sent, where, and who pays for it.

use super::*;

/// `a` takes 1 s, `b` 10 ms; with a 100 ms hedge the answer is `b`'s, at
/// 110 ms, and `a`'s call is dropped.
#[tokio::test(start_paused = true)]
async fn a_slow_idempotent_call_is_answered_by_its_hedge() {
    let fleet = Fleet::new(&[Act::After(1000 * MS), Act::After(10 * MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let start = Instant::now();
    assert_eq!(get_task(&b).await.unwrap(), "b");
    assert_eq!(start.elapsed(), 110 * MS);
    assert_eq!(fleet.calls(), vec![1, 1]);
    for m in b.members().current().iter() {
        assert_eq!(
            m.state.in_flight(),
            0,
            "{:?}: the loser was dropped",
            m.endpoint
        );
    }
}

/// An answer before the delay sends no copy.
#[tokio::test(start_paused = true)]
async fn an_answer_within_the_delay_is_not_hedged() {
    let fleet = Fleet::new(&[Act::After(99 * MS), Act::After(MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    assert_eq!(get_task(&b).await.unwrap(), "a");
    assert_eq!(fleet.calls(), vec![1, 0]);
}

/// A send without an idempotency key is never hedged, nor a keyed one to a
/// peer not known to honour keys; a keyed one to a peer that does is.
#[tokio::test(start_paused = true)]
async fn only_sends_that_carry_an_honoured_key_are_hedged() {
    let key = a2a_protocol_types::idempotency::IDEMPOTENCY_METADATA_KEY;
    let keyed = serde_json::json!({"message": {"metadata": {key: "k-0123456789abcdef"}}});
    let unkeyed = serde_json::json!({"message": {}});
    for (params, honours, copies) in [
        (unkeyed.clone(), true, 0),
        (keyed.clone(), false, 0),
        (keyed, true, 1),
    ] {
        let fleet = Fleet::new(&[Act::After(1000 * MS), Act::After(10 * MS)], None);
        let b = fleet.balancer(&hedged(100 * MS), honours);
        call(&b, "SendMessage", params).await.unwrap();
        assert_eq!(fleet.calls()[1], copies, "honours {honours}");
    }
}

/// When the first call fails after its copy was sent, the copy's answer is
/// returned; when both fail, the first call's error.
#[tokio::test(start_paused = true)]
async fn a_failure_waits_for_the_other_call() {
    let fleet = Fleet::new(&[Act::FailAfter(200 * MS), Act::After(300 * MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let start = Instant::now();
    assert_eq!(get_task(&b).await.unwrap(), "b");
    assert_eq!(start.elapsed(), 400 * MS);

    let fleet = Fleet::new(&[Act::FailAfter(500 * MS), Act::FailAfter(MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let err = get_task(&b).await.expect_err("both failed");
    assert!(
        matches!(&err, ClientError::UnexpectedStatus { body, .. } if body == "a"),
        "{err:?}"
    );
    let fleet = Fleet::new(&[Act::After(500 * MS), Act::FailAfter(MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let start = Instant::now();
    assert_eq!(get_task(&b).await.unwrap(), "a");
    assert_eq!(start.elapsed(), 500 * MS);
}

/// A budget with one hedge in reserve and nothing earned pays for one copy.
#[tokio::test(start_paused = true)]
async fn hedges_are_paid_from_the_budget() {
    let fleet = Fleet::new(&[Act::After(1000 * MS), Act::After(1000 * MS)], None);
    let policy = BalancePolicy::new().with_hedge(
        Hedge::after(100 * MS).with_budget(RetryBudget::new(0.0, 0.0).with_reserve(1.0)),
    );
    let b = fleet.balancer(&policy, false);
    get_task(&b).await.unwrap();
    assert_eq!(fleet.calls(), vec![1, 1]);
    get_task(&b).await.unwrap();
    get_task(&b).await.unwrap();
    assert_eq!(fleet.calls().iter().sum::<usize>(), 4, "one copy only");
}

/// Each call earns its share of a copy: at half a copy per call with a
/// reserve of one, the first slow call is copied (the reserve), the second
/// is not (half earned), the third is (a whole one).
#[tokio::test(start_paused = true)]
async fn calls_earn_their_copies() {
    let fleet = Fleet::new(&[Act::After(1000 * MS), Act::After(1000 * MS)], None);
    let policy = BalancePolicy::new().with_hedge(
        Hedge::after(100 * MS).with_budget(RetryBudget::new(0.5, 0.0).with_reserve(1.0)),
    );
    let b = fleet.balancer(&policy, false);
    let mut sent = Vec::new();
    for _ in 0..3 {
        get_task(&b).await.unwrap();
        sent.push(fleet.calls().iter().sum::<usize>());
    }
    assert_eq!(sent, [2, 3, 5]);
}

/// The copy goes to another endpoint even when the first is still the
/// least loaded.
#[tokio::test(start_paused = true)]
async fn the_copy_goes_elsewhere() {
    let fleet = Fleet::new(&[Act::After(MS), Act::After(MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    get_task(&b).await.unwrap();
    for act in &fleet.acts {
        *act.lock().unwrap() = Act::After(1000 * MS);
    }
    let set = b.members().current();
    let held = [
        set[0].state.enter(crate::origin::Ticket::Normal).unwrap(),
        set[0].state.enter(crate::origin::Ticket::Normal).unwrap(),
    ];
    // a has two calls in flight, b none: the call goes to b, its copy to a.
    get_task(&b).await.unwrap();
    assert_eq!(fleet.calls(), vec![2, 1]);
    drop(held);
}

/// One endpoint has nowhere to send a copy, and a stream is never hedged.
#[tokio::test(start_paused = true)]
async fn no_copy_without_a_second_endpoint_or_for_a_stream() {
    let fleet = Fleet::new(&[Act::After(1000 * MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let start = Instant::now();
    get_task(&b).await.unwrap();
    assert_eq!(start.elapsed(), 1000 * MS);
    assert_eq!(fleet.calls(), vec![1]);

    let fleet = Fleet::new(&[Act::After(MS), Act::After(MS)], None);
    let b = fleet.balancer(&hedged(MS), false);
    let stream = b
        .send_streaming_request("SubscribeToTask", serde_json::json!({}), &HashMap::new())
        .await
        .unwrap();
    assert_eq!(fleet.calls(), vec![1, 0]);
    // The open stream counts in flight until it is dropped.
    let set = b.members().current();
    assert_eq!(set[0].state.in_flight(), 1);
    drop(stream);
    assert_eq!(set[0].state.in_flight(), 0);
}
