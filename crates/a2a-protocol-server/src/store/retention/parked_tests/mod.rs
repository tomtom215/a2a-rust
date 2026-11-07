// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `RetentionPolicy::parked_max_age` (ADR 0056, amending ADR 0011): the parked
//! sweep of each persistent store. The scenarios are written once, against a
//! [`Harness`], and run on all four stores.
//!
//! Age is simulated by writing `updated_at`, as the terminal sweep's tests do:
//! the database's clock is the one that measures it, and a policy in days
//! cannot be waited out. The exact bound, which a moving clock cannot reach, is
//! pinned beside each backend's own statement (`sqlite.rs`, `postgres.rs`).

// Scenarios over a generic store: the futures are awaited on the test's own runtime.
#![allow(clippy::future_not_send)]

#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;

use std::time::Duration;

use a2a_protocol_types::task::TaskState;

use super::{PARKED_STATES, PurgeReport, RetentionPolicy, parked_state_labels};

const HOUR: i64 = 3_600;

fn policy() -> RetentionPolicy {
    RetentionPolicy::new(Duration::from_secs(HOUR.unsigned_abs()))
}

/// What the scenarios need of a store: rows to write, age and count, a sweep to
/// run, a turn claim to hold and a log to look at.
pub(super) trait Harness {
    async fn save(&self, id: &str, state: TaskState);
    /// Makes the task's last update `seconds` old, by the database's clock.
    async fn age(&self, id: &str, seconds: i64);
    async fn ids(&self) -> Vec<String>;
    async fn purge(&self, policy: &RetentionPolicy) -> PurgeReport;
    /// A turn claim on `id`: unexpired, or expired an hour ago.
    async fn claim(&self, id: &str, live: bool);
    async fn append(&self, id: &str, seq: u64);
    async fn events(&self, id: &str) -> i64;
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// Nothing parked is deleted unless the policy says so, however old.
pub(super) async fn default_keeps_every_parked_task(h: &impl Harness) {
    for (id, state) in [
        ("input", TaskState::InputRequired),
        ("auth", TaskState::AuthRequired),
    ] {
        h.save(id, state).await;
        h.age(id, 10 * 365 * 24 * HOUR).await;
    }
    let report = h.purge(&policy()).await;
    assert_eq!(report.parked_tasks_deleted, 0);
    assert_eq!(report.tasks_deleted, 0);
    assert_eq!(h.ids().await, names(&["auth", "input"]));
}

/// With the opt-in: a parked task older than the age goes, whichever parked
/// state it is in, and a younger one, a working one and a finished one do what
/// they did before (the finished one is counted on its own field).
pub(super) async fn opt_in_deletes_only_old_parked_tasks(h: &impl Harness) {
    for (id, state, age) in [
        ("old-input", TaskState::InputRequired, 2 * HOUR),
        ("old-auth", TaskState::AuthRequired, 2 * HOUR),
        ("new-input", TaskState::InputRequired, HOUR / 2),
        ("old-working", TaskState::Working, 2 * HOUR),
        ("old-submitted", TaskState::Submitted, 2 * HOUR),
        ("old-done", TaskState::Completed, 2 * HOUR),
        ("new-done", TaskState::Completed, 10),
    ] {
        h.save(id, state).await;
        h.age(id, age).await;
    }
    let report = h
        .purge(&policy().with_parked_max_age(Duration::from_hours(1)))
        .await;
    assert_eq!(report.parked_tasks_deleted, 2);
    assert_eq!(report.tasks_deleted, 1);
    assert!(report.complete);
    assert_eq!(
        h.ids().await,
        names(&["new-done", "new-input", "old-submitted", "old-working"])
    );
}

/// The age is its own: parked tasks are kept for the parked age even when the
/// terminal age is shorter, and the other way round.
pub(super) async fn the_two_ages_are_independent(h: &impl Harness) {
    for (id, state, age) in [
        ("p", TaskState::InputRequired, 2 * HOUR),
        ("t", TaskState::Completed, 2 * HOUR),
    ] {
        h.save(id, state).await;
        h.age(id, age).await;
    }
    let long = policy().with_parked_max_age(Duration::from_hours(10));
    let report = h.purge(&long).await;
    assert_eq!(
        (report.tasks_deleted, report.parked_tasks_deleted),
        (1, 0),
        "the terminal task is past its age, the parked one is not past its own"
    );
    assert_eq!(h.ids().await, names(&["p"]));
}

/// Either side of the age, by the database's clock: a second short is kept, a
/// second past goes. (The exact bound is pinned per backend.)
pub(super) async fn a_second_either_side_of_the_age(h: &impl Harness) {
    h.save("short", TaskState::InputRequired).await;
    h.age("short", HOUR - 5).await;
    h.save("past", TaskState::AuthRequired).await;
    h.age("past", HOUR + 5).await;
    let report = h
        .purge(&policy().with_parked_max_age(Duration::from_hours(1)))
        .await;
    assert_eq!(report.parked_tasks_deleted, 1);
    assert_eq!(h.ids().await, names(&["short"]));
}

/// A task a turn is running on is not abandoned: a live claim keeps it, an
/// expired one does not.
pub(super) async fn a_live_claim_keeps_a_parked_task(h: &impl Harness) {
    for id in ["claimed", "lapsed", "unclaimed"] {
        h.save(id, TaskState::InputRequired).await;
        h.age(id, 2 * HOUR).await;
    }
    h.claim("claimed", true).await;
    h.claim("lapsed", false).await;
    let report = h
        .purge(&policy().with_parked_max_age(Duration::from_hours(1)))
        .await;
    assert_eq!(report.parked_tasks_deleted, 2);
    assert_eq!(h.ids().await, names(&["claimed"]));
}

/// A sweep that has a batch budget stops at it, says so, and the next one
/// finishes: the batching is the terminal sweep's.
pub(super) async fn batches_are_bounded_and_reported(h: &impl Harness) {
    for i in 0..5 {
        let id = format!("p{i}");
        h.save(&id, TaskState::InputRequired).await;
        h.age(&id, 2 * HOUR).await;
    }
    let bounded = policy()
        .with_parked_max_age(Duration::from_hours(1))
        .with_batch_size(2)
        .with_max_batches(1);
    let first = h.purge(&bounded).await;
    assert_eq!(first.parked_tasks_deleted, 2);
    assert_eq!(first.batches, 1);
    assert!(!first.complete, "out of budget with work left");
    let second = h.purge(&bounded.clone().with_max_batches(10)).await;
    assert_eq!(second.parked_tasks_deleted, 3);
    assert_eq!(
        second.batches, 2,
        "two batches of 2 and 1; the empty one is not counted"
    );
    assert!(second.complete);
    assert_eq!(h.ids().await, names(&[]));
}

/// The task's event log goes with it.
pub(super) async fn the_log_goes_with_the_task(h: &impl Harness) {
    for id in ["gone", "kept"] {
        h.save(id, TaskState::InputRequired).await;
        h.append(id, 1).await;
        h.append(id, 2).await;
    }
    h.age("gone", 2 * HOUR).await;
    let report = h
        .purge(&policy().with_parked_max_age(Duration::from_hours(1)))
        .await;
    assert_eq!(report.parked_tasks_deleted, 1);
    assert_eq!(h.events("gone").await, 0);
    assert_eq!(h.events("kept").await, 2);
}

// ── No database ──────────────────────────────────────────────────────────────

/// The parked states are exactly the interrupted ones, for every variant the
/// protocol defines: a new parked state would otherwise never be swept.
#[test]
fn the_parked_states_are_the_interrupted_ones() {
    for state in TaskState::ALL {
        assert_eq!(
            PARKED_STATES.contains(&state),
            state.is_interrupted(),
            "{state}"
        );
    }
    assert_eq!(
        PARKED_STATES.len(),
        TaskState::ALL.iter().filter(|s| s.is_interrupted()).count()
    );
}

#[test]
fn the_labels_are_the_stored_spellings() {
    assert_eq!(
        parked_state_labels(),
        [
            TaskState::InputRequired.to_string(),
            TaskState::AuthRequired.to_string()
        ]
    );
    assert_eq!(parked_state_labels()[0], "TASK_STATE_INPUT_REQUIRED");
}

#[test]
fn the_parked_age_is_unset_by_default_and_the_setter_sets_only_it() {
    let base = RetentionPolicy::new(Duration::from_secs(60));
    assert_eq!(base.parked_max_age, None);
    let set = base.clone().with_parked_max_age(Duration::from_secs(7));
    assert_eq!(set.parked_max_age, Some(Duration::from_secs(7)));
    assert_eq!(set.terminal_max_age, base.terminal_max_age);
    assert_eq!(set.batch_size, base.batch_size);
    assert_eq!(set.max_batches, base.max_batches);
    assert_eq!(PurgeReport::default().parked_tasks_deleted, 0);
}

/// A key must outlive the task it names, the parked ones included: the key age
/// is raised to the parked age as it is to the terminal one, and only then.
#[test]
fn a_key_age_is_never_below_the_parked_age() {
    let day = Duration::from_hours(24);
    let policy = RetentionPolicy::new(Duration::from_secs(60))
        .with_idempotency_key_max_age(Some(day))
        .with_parked_max_age(Duration::from_hours(30 * 24));
    assert_eq!(
        policy.effective_idempotency_key_max_age(),
        Some(Duration::from_hours(30 * 24))
    );
    let shorter = policy.clone().with_parked_max_age(Duration::from_hours(2));
    assert_eq!(shorter.effective_idempotency_key_max_age(), Some(day));
    let none = policy.with_idempotency_key_max_age(None);
    assert_eq!(none.effective_idempotency_key_max_age(), None);
    // Unset: the clamp is the terminal age alone, as before.
    let unset =
        RetentionPolicy::new(Duration::from_hours(48)).with_idempotency_key_max_age(Some(day));
    assert_eq!(
        unset.effective_idempotency_key_max_age(),
        Some(Duration::from_hours(48))
    );
}
