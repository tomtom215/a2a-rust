// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The gate's rules, each pinned: capacity, reservations, the bounded wait,
//! the budget, priority, weighted rounds, and a waiter that stops waiting.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

use super::*;
use crate::admission::TenantShare;

const MS: fn(u64) -> Duration = Duration::from_millis;

fn load(gate: &AdmissionGate) -> (usize, usize) {
    let load = gate.load();
    (load.running, load.queued)
}

/// Lets every spawned task run as far as it can, one millisecond of paused
/// time at a time, so tasks queue in the order they were spawned.
async fn settle() {
    tokio::time::sleep(MS(1)).await;
}

/// Capacity slots and no more; with no queue the next send is refused at
/// once; a released slot is free again.
#[tokio::test(start_paused = true)]
async fn the_capacity_is_held_and_a_released_slot_is_free_again() {
    let gate = AdmissionGate::new(Admission::new(2));
    let a = gate.acquire("t", None).await.unwrap();
    let _b = gate.acquire("u", None).await.unwrap();
    assert_eq!(load(&gate), (2, 0));
    assert_eq!(gate.acquire("t", None).await.unwrap_err(), Refusal::Full);
    drop(a);
    assert_eq!(load(&gate), (1, 0));
    let _c = gate.acquire("t", None).await.unwrap();
    assert_eq!(gate.load().capacity, 2);
}

/// Capacity 3, `a` reserving 2: one shared slot. `b` takes it and is then
/// refused; `a` still takes both of its own, and a third only once the shared
/// one is free; giving back `a`'s shared slot lets `b` in again.
#[tokio::test(start_paused = true)]
async fn a_reservation_is_kept_for_its_tenant() {
    let gate =
        AdmissionGate::new(Admission::new(3).with_tenant("a", TenantShare::new().with_reserved(2)));
    let b1 = gate.acquire("b", None).await.unwrap();
    assert_eq!(gate.acquire("b", None).await.unwrap_err(), Refusal::Full);
    let a1 = gate.acquire("a", None).await.unwrap();
    let _a2 = gate.acquire("a", None).await.unwrap();
    assert_eq!(gate.acquire("a", None).await.unwrap_err(), Refusal::Full);
    drop(b1);
    let a3 = gate.acquire("a", None).await.unwrap();
    assert_eq!(load(&gate), (3, 0));
    assert_eq!(gate.acquire("b", None).await.unwrap_err(), Refusal::Full);
    // `a` holds three, one of them shared. Whichever it gives back, it is
    // then within its reservation, so the shared slot is free for `b`.
    drop(a1);
    let _b2 = gate.acquire("b", None).await.unwrap();
    drop(a3);
    assert_eq!(load(&gate), (2, 0));
    assert_eq!(gate.acquire("b", None).await.unwrap_err(), Refusal::Full);
}

fn queued(capacity: usize, max_queued: usize, max_wait: Duration) -> AdmissionGate {
    AdmissionGate::new(
        Admission::new(capacity)
            .with_max_queued(max_queued)
            .with_max_wait(max_wait),
    )
}

/// A send waits for a slot and is admitted the moment one is given back;
/// the queue holds `max_queued` sends and refuses the next.
#[tokio::test(start_paused = true)]
async fn a_send_waits_for_a_slot_within_the_queue_bound() {
    let gate = queued(1, 1, Duration::from_secs(1));
    let held = gate.acquire("t", None).await.unwrap();
    let started = Instant::now();
    let waiter = {
        let gate = gate.clone();
        tokio::spawn(async move { gate.acquire("t", None).await.map(|_| Instant::now()) })
    };
    settle().await;
    assert_eq!(load(&gate), (1, 1));
    assert_eq!(gate.acquire("u", None).await.unwrap_err(), Refusal::Full);
    tokio::time::sleep(MS(299)).await;
    drop(held);
    let admitted_at = waiter.await.unwrap().unwrap();
    assert_eq!(admitted_at - started, MS(300));
    assert_eq!(load(&gate), (0, 0), "the waiter's permit was dropped");
}

/// No slot within `max_wait`: refused `Waited` at exactly that bound, and
/// the queue is empty again.
#[tokio::test(start_paused = true)]
async fn a_wait_ends_at_max_wait() {
    let gate = queued(1, 4, MS(800));
    let _held = gate.acquire("t", None).await.unwrap();
    let started = Instant::now();
    let refused = gate.acquire("t", None).await.unwrap_err();
    assert_eq!(refused, Refusal::Waited(MS(800)));
    assert_eq!(started.elapsed(), MS(800));
    assert_eq!(load(&gate), (1, 0));
}

/// A budget shorter than `max_wait` bounds the wait and ends it as
/// `Budget`; one with nothing left is refused at once; one equal to
/// `max_wait` ends as `Waited`.
#[tokio::test(start_paused = true)]
async fn the_budget_bounds_the_wait() {
    let gate = queued(1, 4, Duration::from_secs(1));
    let _held = gate.acquire("t", None).await.unwrap();
    let started = Instant::now();
    assert_eq!(
        gate.acquire("t", Some(MS(200))).await.unwrap_err(),
        Refusal::Budget
    );
    assert_eq!(started.elapsed(), MS(200));
    assert_eq!(
        gate.acquire("t", Some(Duration::ZERO)).await.unwrap_err(),
        Refusal::Budget
    );
    assert_eq!(started.elapsed(), MS(200), "refused without waiting");
    assert_eq!(
        gate.acquire("t", Some(Duration::from_secs(1)))
            .await
            .unwrap_err(),
        Refusal::Waited(Duration::from_secs(1))
    );
    assert_eq!(
        gate.acquire("t", Some(Duration::from_secs(5)))
            .await
            .unwrap_err(),
        Refusal::Waited(Duration::from_secs(1))
    );
    assert_eq!(load(&gate), (1, 0));
}

/// A queue configured with no wait waits for nothing.
#[tokio::test(start_paused = true)]
async fn a_queue_with_no_wait_refuses() {
    let gate = queued(1, 4, Duration::ZERO);
    let _held = gate.acquire("t", None).await.unwrap();
    assert_eq!(gate.acquire("t", None).await.unwrap_err(), Refusal::Full);
}

/// Spawns one waiter per label, in order, each recording its label when
/// admitted and giving its slot straight back.
async fn queue_in_order(
    gate: &AdmissionGate,
    labels: &[&'static str],
) -> Arc<Mutex<Vec<&'static str>>> {
    let order = Arc::new(Mutex::new(Vec::new()));
    for label in labels {
        let gate = gate.clone();
        let order = Arc::clone(&order);
        let tenant = label.trim_end_matches(char::is_numeric).to_owned();
        let label = *label;
        tokio::spawn(async move {
            let permit = gate.acquire(&tenant, None).await.unwrap();
            order.lock().unwrap().push(label);
            drop(permit);
        });
        settle().await;
    }
    order
}

/// The higher priority is served first, though it queued later.
#[tokio::test(start_paused = true)]
async fn a_higher_priority_is_served_first() {
    let gate = AdmissionGate::new(
        Admission::new(1)
            .with_max_queued(8)
            .with_max_wait(Duration::from_secs(10))
            .with_tenant("high", TenantShare::new().with_priority(1)),
    );
    let held = gate.acquire("x", None).await.unwrap();
    let order = queue_in_order(&gate, &["low1", "low2", "high1", "high2"]).await;
    drop(held);
    settle().await;
    assert_eq!(*order.lock().unwrap(), ["high1", "high2", "low1", "low2"]);
}

/// Within one priority, `a` (weight 2) and `b` (weight 1) take turns, two of
/// `a`'s to one of `b`'s, and `b` has the rest once `a` has none waiting.
#[tokio::test(start_paused = true)]
async fn tenants_take_turns_by_weight() {
    let gate = AdmissionGate::new(
        Admission::new(1)
            .with_max_queued(16)
            .with_max_wait(Duration::from_secs(10))
            .with_tenant("a", TenantShare::new().with_weight(2)),
    );
    let held = gate.acquire("x", None).await.unwrap();
    let order = queue_in_order(&gate, &["a1", "a2", "a3", "a4", "b1", "b2", "b3", "b4"]).await;
    assert_eq!(load(&gate), (1, 8));
    drop(held);
    settle().await;
    assert_eq!(
        *order.lock().unwrap(),
        ["a1", "a2", "b1", "a3", "a4", "b2", "b3", "b4"]
    );
    assert_eq!(load(&gate), (0, 0));
}

/// A freed slot passes over a waiter that may not take it (its tenant is at
/// its reservation and the shared part is full) for one that may.
#[tokio::test(start_paused = true)]
async fn a_slot_passes_over_a_waiter_that_may_not_take_it() {
    let gate = AdmissionGate::new(
        Admission::new(2)
            .with_max_queued(8)
            .with_max_wait(Duration::from_secs(10))
            .with_tenant("a", TenantShare::new().with_reserved(1)),
    );
    let _shared = gate.acquire("b", None).await.unwrap();
    let reserved = gate.acquire("a", None).await.unwrap();
    let order = queue_in_order(&gate, &["b1", "a1"]).await;
    drop(reserved);
    settle().await;
    assert_eq!(*order.lock().unwrap(), ["a1"]);
    assert_eq!(load(&gate), (1, 1), "b1 still waits for a shared slot");
}

/// A waiter whose request is dropped leaves the queue; one dropped just
/// after it was granted a slot, before it ran, gives the slot back.
#[tokio::test(start_paused = true)]
async fn a_waiter_that_stops_waiting_leaves_nothing_behind() {
    let gate = queued(1, 4, Duration::from_secs(10));
    let held = gate.acquire("t", None).await.unwrap();
    let gone = {
        let gate = gate.clone();
        tokio::spawn(async move { gate.acquire("t", None).await.map(drop) })
    };
    settle().await;
    assert_eq!(load(&gate), (1, 1));
    gone.abort();
    settle().await;
    assert_eq!(
        load(&gate),
        (1, 0),
        "an abandoned waiter is out of the queue"
    );

    let granted_then_gone = {
        let gate = gate.clone();
        tokio::spawn(async move { gate.acquire("t", None).await.map(drop) })
    };
    settle().await;
    // The grant and the abort land before the waiter runs again.
    drop(held);
    assert_eq!(load(&gate), (1, 0), "granted to the waiter");
    granted_then_gone.abort();
    settle().await;
    assert_eq!(load(&gate), (0, 0), "and given back when it stopped");
    let _again = gate.acquire("t", None).await.unwrap();
}

/// The `Debug` forms name the tenant and the load, nothing else.
#[tokio::test]
async fn the_debug_forms_name_the_tenant_and_the_load() {
    let gate = AdmissionGate::new(Admission::new(1));
    let permit = gate.acquire("acme", None).await.unwrap();
    assert_eq!(
        format!("{permit:?}"),
        r#"AdmissionPermit { tenant: "acme", .. }"#
    );
    let shown = format!("{gate:?}");
    assert!(
        shown.ends_with("load: AdmissionLoad { running: 1, queued: 0, capacity: 1 } }"),
        "{shown}"
    );
    assert!(
        shown.starts_with("AdmissionGate { config: Admission { capacity: 1,"),
        "{shown}"
    );
}

/// A waiter that stops waiting leaves no empty place in its class's rotation
/// for the next freed slot to stop at: the waiter behind it is admitted.
#[tokio::test(start_paused = true)]
async fn an_abandoned_waiter_does_not_stall_the_one_behind_it() {
    let gate = queued(1, 4, Duration::from_secs(10));
    let held = gate.acquire("x", None).await.unwrap();
    let gone = {
        let gate = gate.clone();
        tokio::spawn(async move { gate.acquire("a", None).await.map(drop) })
    };
    settle().await;
    let behind = {
        let gate = gate.clone();
        tokio::spawn(async move { gate.acquire("b", None).await.map(|_| Instant::now()) })
    };
    settle().await;
    gone.abort();
    settle().await;
    assert_eq!(load(&gate), (1, 1));
    let freed = Instant::now();
    drop(held);
    assert_eq!(
        behind.await.unwrap().unwrap(),
        freed,
        "admitted when the slot was freed"
    );
}

/// Two waiters of one tenant are told apart by their ids, not by their
/// places: when the newer one stops waiting, the older one keeps its place
/// and is the one admitted when the slot is freed. (If the ids repeated, the
/// newer one's leaving would take the older one's entry out of the queue.)
#[tokio::test(start_paused = true)]
async fn the_waiter_that_stops_waiting_is_the_one_that_leaves_the_queue() {
    let gate = queued(1, 4, Duration::from_secs(10));
    let held = gate.acquire("t", None).await.unwrap();
    let older = {
        let gate = gate.clone();
        tokio::spawn(async move { gate.acquire("t", None).await.map(|_| Instant::now()) })
    };
    settle().await;
    let newer = {
        let gate = gate.clone();
        tokio::spawn(async move { gate.acquire("t", None).await.map(drop) })
    };
    settle().await;
    assert_eq!(load(&gate), (1, 2));
    newer.abort();
    settle().await;
    assert_eq!(load(&gate), (1, 1), "only the newer waiter left");
    let freed = Instant::now();
    drop(held);
    assert_eq!(
        older.await.unwrap().unwrap(),
        freed,
        "the older waiter was admitted when the slot was freed"
    );
}
