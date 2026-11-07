// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `TaskStoreConfig::parked_task_ttl` (ADR 0056): a parked task that nobody
//! answered for the TTL becomes evictable like a finished one, and not before.
//!
//! The clock is a parameter at every site that compares ages, so each bound is
//! pinned at one nanosecond either side of it: `now` is a fixed instant far
//! enough ahead of the real clock that no entry here depends on how long the
//! machine has been up. The default (no TTL) is tested beside each rule, since
//! "nothing expires" is the behaviour every earlier release had.

use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_types::task::{TaskId, TaskState};

use super::fixtures::{config, ids, task};
use super::{EvictionPasses, InMemoryTaskStore, StoreData, TaskStoreConfig};
use crate::store::TaskStore;
use crate::store::task_store::TaskBudget;

use TaskState::{AuthRequired, Completed, InputRequired, Working};

const NS: Duration = Duration::from_nanos(1);
const TTL: Duration = Duration::from_secs(60);

/// A fixed "now" a million seconds past the real clock: instants before it
/// exist whatever the machine's uptime.
fn now() -> Instant {
    static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *BASE.get_or_init(|| Instant::now() + Duration::from_secs(1_000_000))
}

/// A store holding `t0`, `t1`, … in that order, each last written at the
/// instant given.
fn data_of(entries: &[(TaskState, Instant)]) -> StoreData {
    let mut data = StoreData::with_capacity(entries.len());
    for (i, (state, written)) in entries.iter().enumerate() {
        data.insert(
            TaskId::new(format!("t{i}")),
            task(&format!("t{i}"), *state),
            *written,
        );
    }
    data
}

/// `t` less `d`; the clock here is far enough ahead that it exists.
fn before(t: Instant, d: Duration) -> Instant {
    t.checked_sub(d)
        .expect("the test clock is a million seconds ahead")
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

// ── The setting ──────────────────────────────────────────────────────────────

/// Unset by default, and the setter sets it and nothing else.
#[test]
fn the_default_expires_nothing_and_the_setter_sets_only_the_ttl() {
    let default = TaskStoreConfig::default();
    assert_eq!(default.parked_task_ttl, None);
    assert_eq!(default.parked_cutoff(), None, "no TTL, no cutoff");

    let set = TaskStoreConfig::default().with_parked_task_ttl(TTL);
    assert_eq!(set.parked_task_ttl, Some(TTL));
    assert_eq!(set.task_ttl, default.task_ttl);
    assert_eq!(set.max_capacity, default.max_capacity);
}

/// The cutoff is the clock less the TTL, and a TTL longer than the clock has
/// run reaches back to no instant at all: nothing can have been parked that
/// long, so nothing expires (and nothing panics).
#[test]
fn the_cutoff_is_now_less_the_ttl_and_none_past_the_clock() {
    let before = Instant::now();
    let cutoff = TaskStoreConfig::default()
        .with_parked_task_ttl(Duration::ZERO)
        .parked_cutoff()
        .expect("a zero TTL reaches back to now");
    assert!(before <= cutoff && cutoff <= Instant::now());

    let huge = TaskStoreConfig::default().with_parked_task_ttl(Duration::MAX);
    assert_eq!(huge.parked_cutoff(), None);
}

// ── The TTL pass ─────────────────────────────────────────────────────────────

/// The pass takes a parked task aged at least the TTL, at the bound and one
/// nanosecond past it, and keeps one a nanosecond short. Finished tasks and
/// working ones of the same age are not this pass's: a working task is kept
/// however old, and a finished one is `task_ttl`'s.
#[test]
fn the_ttl_pass_takes_parked_tasks_at_least_the_ttl_old() {
    let mut data = data_of(&[
        (InputRequired, before(now(), TTL) + NS),
        (InputRequired, before(now(), TTL)),
        (AuthRequired, before(now(), TTL + NS)),
        (Working, before(now(), TTL + NS)),
        (Completed, before(now(), TTL + NS)),
    ]);
    InMemoryTaskStore::evict_expired_parked(&mut data, TTL, now());
    assert_eq!(ids(&data), names(&["t0", "t3", "t4"]));
    assert_eq!(data.parked_evictions, 2);
    assert_eq!(data.live_evictions, 0, "a parked task is not a lost one");
}

/// A TTL longer than the clock has run removes nothing and does not panic.
#[test]
fn a_ttl_beyond_the_clock_removes_nothing() {
    let mut data = data_of(&[(InputRequired, before(now(), Duration::from_secs(1_000_000)))]);
    InMemoryTaskStore::evict_expired_parked(&mut data, Duration::MAX, now());
    assert_eq!(ids(&data), names(&["t0"]));
    assert_eq!(data.parked_evictions, 0);
}

/// Removing goes through `StoreData::remove`, so no index keeps the id.
#[test]
fn an_expired_parked_task_leaves_no_index_entry() {
    let mut data = data_of(&[(InputRequired, before(now(), TTL)), (Working, now())]);
    InMemoryTaskStore::evict_expired_parked(&mut data, TTL, now());
    assert_eq!(data.entries.len(), 1);
    assert_eq!(data.order_index.len(), 1);
    assert_eq!(data.expiry_index.len(), 1);
    assert_eq!(
        data.context_index
            .values()
            .map(std::collections::BTreeMap::len)
            .sum::<usize>(),
        1
    );
}

/// `evict` runs the parked pass on the TTL schedule only, and only with a TTL:
/// a capacity-only sweep leaves an expired parked task (so it costs the
/// full-store hot path nothing), and a store with no `parked_task_ttl` never
/// loses one, whatever its age.
#[test]
fn evict_runs_the_parked_pass_on_the_ttl_schedule_and_only_when_set() {
    let aged = || data_of(&[(InputRequired, Instant::now()), (Working, Instant::now())]);
    let cfg = |ttl: Option<Duration>| {
        let mut c = config(None, None, 0);
        c.parked_task_ttl = ttl;
        c
    };
    let ttl_only = EvictionPasses {
        ttl: true,
        capacity: false,
    };
    let capacity_only = EvictionPasses {
        ttl: false,
        capacity: true,
    };

    // Zero TTL: everything parked is already expired.
    let mut data = aged();
    InMemoryTaskStore::evict(&mut data, &cfg(Some(Duration::ZERO)), ttl_only);
    assert_eq!(ids(&data), names(&["t1"]), "the TTL pass takes it");
    assert_eq!(data.parked_evictions, 1);

    let mut data = aged();
    InMemoryTaskStore::evict(&mut data, &cfg(Some(Duration::ZERO)), capacity_only);
    assert_eq!(data.len(), 2, "a capacity-only sweep does not run it");

    let mut data = aged();
    InMemoryTaskStore::evict(&mut data, &cfg(None), EvictionPasses::all());
    assert_eq!(data.len(), 2, "no TTL set: nothing parked expires");
    assert_eq!(data.parked_evictions, 0);
}

/// The removal is logged, and a pass that removes nothing is silent.
#[cfg(feature = "tracing")]
#[test]
fn the_ttl_pass_warns_when_it_removes_parked_tasks_and_only_then() {
    let mut data = data_of(&[
        (InputRequired, before(now(), TTL)),
        (AuthRequired, before(now(), TTL)),
    ]);
    let warned = crate::handler::warnings_during(|| {
        InMemoryTaskStore::evict_expired_parked(&mut data, TTL, now());
    });
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(warned[0].starts_with("parked tasks expired"), "{warned:?}");

    let quiet = crate::handler::warnings_during(|| {
        InMemoryTaskStore::evict_expired_parked(&mut data, TTL, now());
    });
    assert!(quiet.is_empty(), "nothing was left to expire: {quiet:?}");
}

// ── The capacity pass ────────────────────────────────────────────────────────

/// A parked task is taken for room once it has expired, at the bound and past
/// it, and not a nanosecond before: the expired one goes although it is not the
/// oldest in the store.
#[test]
fn an_expired_parked_task_is_taken_for_room_at_the_bound() {
    let cutoff = before(now(), TTL);
    for (written, taken) in [
        (cutoff + NS, None),
        (cutoff, Some("t1")),
        (before(cutoff, NS), Some("t1")),
    ] {
        let mut data = data_of(&[(InputRequired, cutoff + NS), (AuthRequired, written)]);
        InMemoryTaskStore::evict_over_capacity(&mut data, 1, Some(cutoff));
        let want: Vec<String> = names(&["t0", "t1"])
            .into_iter()
            .filter(|n| Some(n.as_str()) != taken)
            .collect();
        assert_eq!(
            ids(&data),
            want,
            "written {:?} before the cutoff",
            cutoff.duration_since(written)
        );
        assert_eq!(data.parked_evictions, u64::from(taken.is_some()));
    }
}

/// An expired parked task counts as finished: it goes before the oldest
/// working task, which is spared and not counted as lost.
#[test]
fn an_expired_parked_task_goes_before_a_working_one() {
    let cutoff = before(now(), TTL);
    let mut data = data_of(&[(Working, now()), (InputRequired, cutoff), (Working, now())]);
    InMemoryTaskStore::evict_over_capacity(&mut data, 2, Some(cutoff));
    assert_eq!(ids(&data), names(&["t0", "t2"]));
    assert_eq!(data.parked_evictions, 1);
    assert_eq!(data.live_evictions, 0);
}

/// Without a cutoff a parked task is never taken, however old: the oldest
/// working one is, as before this setting existed.
#[test]
fn without_a_ttl_the_capacity_pass_never_takes_a_parked_task() {
    let mut data = data_of(&[
        (InputRequired, before(now(), Duration::from_secs(1_000))),
        (Working, now()),
        (Working, now()),
    ]);
    InMemoryTaskStore::evict_over_capacity(&mut data, 2, None);
    assert_eq!(ids(&data), names(&["t0", "t2"]));
    assert_eq!(data.parked_evictions, 0);
    assert_eq!(data.live_evictions, 1);
}

/// One sweep that takes two expired parked tasks and tops up with a working
/// one reports each on its own counter.
#[test]
fn a_mixed_sweep_counts_parked_and_working_victims_apart() {
    let cutoff = before(now(), TTL);
    let mut data = data_of(&[
        (InputRequired, cutoff),
        (Working, now()),
        (AuthRequired, before(cutoff, NS)),
        (Working, now()),
        (Working, now()),
    ]);
    InMemoryTaskStore::evict_over_capacity(&mut data, 2, Some(cutoff));
    assert_eq!(ids(&data), names(&["t3", "t4"]));
    assert_eq!(data.parked_evictions, 2);
    assert_eq!(data.live_evictions, 1);
}

// ── The refusal ──────────────────────────────────────────────────────────────

/// A full store of parked tasks refuses a new one, unless one of them has
/// expired, which the bound decides exactly. Each condition varies alone.
#[test]
fn a_store_full_of_parked_tasks_refuses_unless_one_has_expired() {
    let cutoff = before(now(), TTL);
    let new = TaskId::new("new");
    let full = |oldest: Instant| {
        data_of(&[
            (InputRequired, oldest),
            (AuthRequired, now()),
            (InputRequired, now()),
        ])
    };
    assert!(
        full(cutoff + NS).refuses_for_parked_tasks(&new, Some(3), Some(cutoff)),
        "a nanosecond short of the TTL: still waiting"
    );
    assert!(
        !full(cutoff).refuses_for_parked_tasks(&new, Some(3), Some(cutoff)),
        "at the TTL: expired, so there is something to evict"
    );
    assert!(
        !full(before(cutoff, NS)).refuses_for_parked_tasks(&new, Some(3), Some(cutoff)),
        "past it"
    );
    assert!(
        full(before(cutoff, NS)).refuses_for_parked_tasks(&new, Some(3), None),
        "no TTL: as old as it likes, it is never evicted, so the store refuses"
    );
    // The expired one is not the oldest by position, only by age.
    let data = data_of(&[
        (InputRequired, now()),
        (AuthRequired, cutoff),
        (InputRequired, now()),
    ]);
    assert!(!data.refuses_for_parked_tasks(&new, Some(3), Some(cutoff)));
}

// ── The shared budget ────────────────────────────────────────────────────────

/// A tenant partition under a total cap frees a slot from its oldest finished
/// task, or its oldest expired parked one, and from nothing else.
#[test]
fn a_full_budget_frees_a_slot_from_an_expired_parked_task() {
    let cutoff = before(now(), TTL);
    for (written, freed) in [
        (cutoff + NS, false),
        (cutoff, true),
        (before(cutoff, NS), true),
    ] {
        let mut data = data_of(&[(InputRequired, written), (Working, now())]);
        let budget = Arc::new(TaskBudget::new(2));
        assert!(budget.reserve() && budget.reserve());
        data.budget = Some(Arc::clone(&budget));
        assert_eq!(
            data.admit(&TaskId::new("new"), Some(cutoff)),
            freed,
            "written {:?} before the cutoff",
            cutoff.duration_since(written)
        );
        assert_eq!(data.entries.len(), if freed { 1 } else { 2 });
        assert_eq!(data.parked_evictions, u64::from(freed));
    }
    let mut data = data_of(&[(InputRequired, before(cutoff, NS)), (Working, now())]);
    let budget = Arc::new(TaskBudget::new(2));
    assert!(budget.reserve() && budget.reserve());
    data.budget = Some(budget);
    assert!(
        !data.admit(&TaskId::new("new"), None),
        "no TTL: a parked task is never freed for a slot"
    );
    assert_eq!(data.parked_evictions, 0);
}

/// A finished task is still freed first, and is not counted as parked.
#[test]
fn a_full_budget_still_frees_a_finished_task_and_counts_no_parked_one() {
    let mut data = data_of(&[(Completed, now()), (Working, now())]);
    let budget = Arc::new(TaskBudget::new(2));
    assert!(budget.reserve() && budget.reserve());
    data.budget = Some(budget);
    assert!(data.admit(&TaskId::new("new"), Some(before(now(), TTL))));
    assert_eq!(ids(&data), names(&["t1"]));
    assert_eq!(data.parked_evictions, 0);
}

// ── Through the store ────────────────────────────────────────────────────────

/// With a zero TTL every parked task is already expired: a full store takes a
/// new task by evicting the oldest, where the default refuses, and the loss is
/// on the counter and nowhere else.
#[tokio::test]
async fn a_full_store_of_expired_parked_tasks_takes_a_new_task() {
    let mut cfg = config(Some(2), None, 0);
    cfg.parked_task_ttl = Some(Duration::ZERO);
    let store = InMemoryTaskStore::with_config(cfg);
    store.save(&task("p1", InputRequired)).await.unwrap();
    store.save(&task("p2", AuthRequired)).await.unwrap();

    store
        .save(&task("w", Working))
        .await
        .expect("the parked tasks have expired, so there is room");
    assert!(store.get(&TaskId::new("p1")).await.unwrap().is_none());
    assert!(store.get(&TaskId::new("p2")).await.unwrap().is_some());
    assert!(store.get(&TaskId::new("w")).await.unwrap().is_some());
    assert_eq!(store.parked_evictions().await, 1);
    assert_eq!(store.live_evictions().await, 0);

    store
        .insert_if_absent(&task("w2", Working))
        .await
        .expect("insert_if_absent finds the same room");
    assert_eq!(store.count().await.unwrap(), 2);
    assert_eq!(store.parked_evictions().await, 2);
}

/// The default, through the same store: the full store refuses, and nothing
/// is counted.
#[tokio::test]
async fn without_a_ttl_the_same_store_refuses_and_counts_nothing() {
    let store = InMemoryTaskStore::with_config(config(Some(2), None, 0));
    store.save(&task("p1", InputRequired)).await.unwrap();
    store.save(&task("p2", AuthRequired)).await.unwrap();
    assert!(store.save(&task("w", Working)).await.is_err());
    assert!(store.insert_if_absent(&task("w", Working)).await.is_err());
    assert_eq!(store.parked_evictions().await, 0);
}

/// `run_eviction` is the sweep an operator schedules: it takes the expired
/// parked tasks and leaves the rest.
#[tokio::test]
async fn run_eviction_expires_parked_tasks_when_a_ttl_is_set() {
    let mut cfg = config(None, None, 0);
    cfg.parked_task_ttl = Some(Duration::ZERO);
    let store = InMemoryTaskStore::with_config(cfg);
    store.save(&task("p", InputRequired)).await.unwrap();
    store.save(&task("w", Working)).await.unwrap();
    store.run_eviction().await;
    assert!(store.get(&TaskId::new("p")).await.unwrap().is_none());
    assert!(store.get(&TaskId::new("w")).await.unwrap().is_some());
    assert_eq!(store.parked_evictions().await, 1);

    let kept = InMemoryTaskStore::with_config(config(None, None, 0));
    kept.save(&task("p", InputRequired)).await.unwrap();
    kept.run_eviction().await;
    assert!(kept.get(&TaskId::new("p")).await.unwrap().is_some());
    assert_eq!(kept.parked_evictions().await, 0);
}

/// A TTL longer than the clock has run expires nothing, through the store.
#[tokio::test]
async fn a_ttl_beyond_the_clock_keeps_every_parked_task_in_the_store() {
    let mut cfg = config(Some(1), None, 1);
    cfg.parked_task_ttl = Some(Duration::MAX);
    let store = InMemoryTaskStore::with_config(cfg);
    store.save(&task("p", InputRequired)).await.unwrap();
    assert!(store.save(&task("w", Working)).await.is_err());
    store.run_eviction().await;
    assert!(store.get(&TaskId::new("p")).await.unwrap().is_some());
}
