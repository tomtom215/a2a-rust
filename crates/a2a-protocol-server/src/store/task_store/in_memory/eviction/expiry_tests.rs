// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The TTL pass reads `expiry_index`, so the index has to say exactly what
//! `last_updated` says.
//!
//! The pass used to visit every entry, which made it correct by construction
//! and slow: under the write lock, every `eviction_interval` writes, over up
//! to `max_capacity` entries. It now walks only the prefix of a write-ordered
//! index, which is fast and correct only while that index tracks every write.
//! These tests pin both halves: the boundary is exact, and every path that
//! moves `last_updated` moves the index with it.

use std::time::{Duration, Instant};

use a2a_protocol_types::task::{TaskId, TaskState, TaskStatus};

use super::super::{InMemoryTaskStore, StoreData};
use super::fixtures::task;

const TTL: Duration = Duration::from_secs(5);

/// Every entry has exactly one `expiry_index` key, and it is
/// `(last_updated, order sequence)`. An entry missing from the index is never
/// expired; a stale key expires a task early or names one that is gone.
fn assert_consistent(data: &StoreData) {
    assert_eq!(
        data.expiry_index.len(),
        data.entries.len(),
        "expiry_index and entries disagree on how many tasks there are"
    );
    for (id, entry) in &data.entries {
        assert_eq!(
            data.expiry_index
                .get(&(entry.last_updated, entry.order_key.1)),
            Some(id),
            "entry {id:?} is not indexed under its own last write"
        );
    }
}

fn one(state: TaskState, at: Instant) -> StoreData {
    let mut data = StoreData::with_capacity(1);
    data.insert(TaskId::new("t"), task("t", state), at);
    data
}

/// Written exactly `ttl` ago is expired ("at least `ttl` old"); one
/// nanosecond younger is not. Separates `..=` from `..` on the range and an
/// off-by-one in the cutoff.
#[test]
fn a_terminal_task_expires_exactly_at_ttl_and_not_a_tick_before() {
    let t = Instant::now();

    let mut data = one(TaskState::Completed, t);
    let just_short = (t + TTL)
        .checked_sub(Duration::from_nanos(1))
        .expect("t + TTL is after t");
    InMemoryTaskStore::evict_expired(&mut data, TTL, just_short);
    assert_eq!(
        data.entries.len(),
        1,
        "one nanosecond short of the TTL must survive"
    );

    InMemoryTaskStore::evict_expired(&mut data, TTL, t + TTL);
    assert!(
        data.entries.is_empty(),
        "written exactly TTL ago must expire"
    );
    assert_consistent(&data);
}

/// A task still working is kept however old its last write is, and the walk
/// past it still reaches the expired terminal task behind it.
#[test]
fn an_old_working_task_is_kept_and_does_not_shield_older_terminal_ones() {
    let t = Instant::now();
    let mut data = StoreData::with_capacity(3);
    data.insert(TaskId::new("w"), task("w", TaskState::Working), t);
    data.insert(
        TaskId::new("c"),
        task("c", TaskState::Completed),
        t + Duration::from_secs(1),
    );
    data.insert(
        TaskId::new("fresh"),
        task("fresh", TaskState::Completed),
        t + Duration::from_secs(60),
    );

    InMemoryTaskStore::evict_expired(&mut data, TTL, t + Duration::from_secs(30));

    let mut left: Vec<&str> = data.entries.keys().map(|k| k.0.as_str()).collect();
    left.sort_unstable();
    assert_eq!(left, vec!["fresh", "w"]);
    assert_consistent(&data);
}

/// A rewrite through `insert` moves the task out of the expired prefix.
#[test]
fn a_full_save_resets_the_clock() {
    let t = Instant::now();
    let mut data = one(TaskState::Completed, t);
    data.insert(
        TaskId::new("t"),
        task("t", TaskState::Completed),
        t + Duration::from_secs(10),
    );
    assert_consistent(&data);

    InMemoryTaskStore::evict_expired(&mut data, TTL, t + TTL);
    assert_eq!(
        data.entries.len(),
        1,
        "rewritten at t+10s, it is not TTL old at t+5s"
    );
    InMemoryTaskStore::evict_expired(&mut data, TTL, t + Duration::from_secs(10) + TTL);
    assert!(data.entries.is_empty());
    assert_consistent(&data);
}

/// The status-delta path re-keys too: a task that becomes terminal late is
/// aged from that write, not from its first one.
#[test]
fn a_status_update_resets_the_clock() {
    let t = Instant::now();
    let mut data = one(TaskState::Working, t);
    assert!(data.update_status(
        &TaskId::new("t"),
        TaskStatus::new(TaskState::Completed),
        t + Duration::from_secs(10),
    ));
    assert_consistent(&data);

    InMemoryTaskStore::evict_expired(&mut data, TTL, t + TTL);
    assert_eq!(
        data.entries.len(),
        1,
        "completed at t+10s, it is not TTL old at t+5s"
    );
    InMemoryTaskStore::evict_expired(&mut data, TTL, t + Duration::from_secs(10) + TTL);
    assert!(data.entries.is_empty());
}

/// The artifact-delta path writes through `touch`, which must re-key and
/// leave no stale key behind.
#[test]
fn touch_resets_the_clock_and_leaves_one_key() {
    let t = Instant::now();
    let mut data = one(TaskState::Completed, t);
    data.touch(&TaskId::new("t"), t + Duration::from_secs(10));
    assert_consistent(&data);

    InMemoryTaskStore::evict_expired(&mut data, TTL, t + TTL);
    assert_eq!(
        data.entries.len(),
        1,
        "touched at t+10s, it is not TTL old at t+5s"
    );

    // Touching an absent task is a no-op, not an index entry for nothing.
    data.touch(&TaskId::new("absent"), t);
    assert_consistent(&data);
}

/// Removal takes the index entry with it, so a later pass cannot name a task
/// that is gone.
#[test]
fn remove_drops_the_expiry_key() {
    let t = Instant::now();
    let mut data = one(TaskState::Completed, t);
    assert!(data.remove(&TaskId::new("t")).is_some());
    assert!(data.expiry_index.is_empty());
}

/// A TTL longer than the monotonic clock has run cannot have elapsed for
/// anything, and must not panic computing its cutoff.
#[test]
fn a_ttl_beyond_the_clock_expires_nothing() {
    let t = Instant::now();
    let mut data = one(TaskState::Completed, t);
    InMemoryTaskStore::evict_expired(&mut data, Duration::MAX, t);
    assert_eq!(data.entries.len(), 1);
}
