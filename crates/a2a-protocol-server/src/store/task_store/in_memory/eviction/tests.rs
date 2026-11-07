// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests of the TTL and capacity passes and their triggers.

use super::fixtures::{config, ids, store_of};
use super::*;
use a2a_protocol_types::task::TaskState;

// ── should_evict ─────────────────────────────────────────────────────────

/// With the interval disabled and no capacity limit there is nothing to
/// trigger a sweep, so this must be false.
///
/// This one case separates four mutations at once, because each of them
/// makes the interval arm fire when it should not: returning `true`
/// outright, `interval > 0` becoming `>= 0` or `== 0`, and the `&&`
/// becoming `||`. All three of the latter reach
/// `count.is_multiple_of(0)`, which is `true` for a count of 0.
#[test]
fn should_evict_is_false_when_nothing_triggers_a_sweep() {
    let store = InMemoryTaskStore::with_config(config(None, None, 0));
    assert!(!store.should_evict(0).any());
}

/// The interval arm fires on a write count that is a multiple of it, and it
/// is the arm that drives the TTL pass.
#[test]
fn should_evict_fires_on_the_interval() {
    let store = InMemoryTaskStore::with_config(config(None, None, 2));
    assert!(store.should_evict(0).ttl, "write 0 is a multiple of 2");
}

/// Being over capacity earns the capacity pass and *not* the TTL pass.
///
/// This is the property that keeps a full store cheap. Once the store is at
/// `max_capacity` every subsequent write is over it, so if the two passes
/// were bundled the O(n) TTL scan would run on every write forever — which
/// is what made a `message/send` cost 2.1 ms instead of 65 µs past the
/// 10,000th task. The interval is set high enough here that it cannot be
/// the arm supplying `ttl`.
#[test]
fn over_capacity_does_not_earn_the_ttl_pass() {
    let store = InMemoryTaskStore::with_config(config(Some(3), Some(Duration::from_secs(1)), 0));
    let passes = store.should_evict(4);
    assert!(passes.capacity, "one past capacity must sweep for capacity");
    assert!(
        !passes.ttl,
        "an over-capacity write must not drag the O(n) TTL scan along with it"
    );
}

/// Capacity is a strict overflow check: being exactly at `max_capacity` is
/// not over it. `>=` would sweep a store that is merely full.
#[test]
fn should_evict_treats_capacity_as_a_strict_overflow() {
    // A large interval so the interval arm stays quiet after the first call.
    let store = InMemoryTaskStore::with_config(config(Some(3), None, 100));
    assert!(
        store.should_evict(0).ttl,
        "write 0 is a multiple of any interval"
    );

    assert!(
        !store.should_evict(3).capacity,
        "exactly at capacity is not over it"
    );
    assert!(
        store.should_evict(4).capacity,
        "one past capacity triggers a sweep"
    );
}

// ── evict ────────────────────────────────────────────────────────────────

/// A capacity sweep alone must not expire anything, however stale.
///
/// The companion to `over_capacity_does_not_earn_the_ttl_pass`: that one
/// checks the trigger does not *ask* for the TTL pass, this one checks
/// `evict` honours the answer. Every task here is terminal and far past the
/// TTL, so a sweep that ran the TTL pass would empty the store instead of
/// trimming it to the cap.
#[test]
fn a_capacity_only_sweep_does_not_run_the_ttl_pass() {
    let mut data = store_of(&[TaskState::Completed; 5]);

    InMemoryTaskStore::evict(
        &mut data,
        &config(Some(2), Some(Duration::from_millis(1)), 0),
        EvictionPasses {
            ttl: false,
            capacity: true,
        },
    );

    assert_eq!(
        data.len(),
        2,
        "only the overflow goes; the TTL pass must not have run"
    );
}

/// TTL eviction removes terminal tasks past the TTL and spares in-flight
/// ones of the same age.
#[test]
fn evict_expires_terminal_tasks_only() {
    let mut data = store_of(&[TaskState::Completed, TaskState::Working]);
    InMemoryTaskStore::evict(
        &mut data,
        &config(None, Some(Duration::from_millis(1)), 0),
        EvictionPasses::all(),
    );

    assert_eq!(
        ids(&data),
        vec!["t1".to_string()],
        "the terminal task expires; the working one is kept regardless of age"
    );
}

/// A cancelled sweep must not disable eviction for the life of the process.
///
/// `maybe_evict` claims the slot and then awaits the write lock. Until
/// 2026-08-19 the flag was cleared by a `store(false)` after the sweep, so a
/// future dropped while parked on that lock left it set forever — and both
/// of this store's memory bounds, TTL and capacity, run through here. The
/// result was not a slower sweep but no sweep at all.
///
/// Cancellation is ordinary on this path: `save` is awaited on the request
/// and background paths, and a handler timeout, a dropped client request or
/// a shutdown cancellation drops that future wherever it is.
#[tokio::test]
async fn a_cancelled_sweep_releases_the_eviction_slot() {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    let store = Arc::new(InMemoryTaskStore::with_config(config(Some(1), None, 1)));
    let passes = EvictionPasses {
        ttl: true,
        capacity: true,
    };

    // Hold the write lock so the sweep parks after claiming the slot.
    let guard = store.data.write().await;

    let parked = Arc::clone(&store);
    let handle = tokio::spawn(async move { parked.maybe_evict(passes).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        store.eviction_in_progress.load(Ordering::Acquire),
        "sanity: the parked sweep should be holding the slot"
    );

    // The caller goes away.
    handle.abort();
    let _ = handle.await;
    drop(guard);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(
        !store.eviction_in_progress.load(Ordering::Acquire),
        "a cancelled sweep left the slot claimed; every later sweep is now a \
         no-op and neither the TTL nor the capacity bound will ever run again"
    );

    // And the next sweep must actually get in.
    store.maybe_evict(passes).await;
    assert!(
        !store.eviction_in_progress.load(Ordering::Acquire),
        "the slot must be free again after a completed sweep"
    );
}
