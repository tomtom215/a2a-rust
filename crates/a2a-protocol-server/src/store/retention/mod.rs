// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Retention for the persistent task stores.
//!
//! # The settled policy
//!
//! **A persistent store deletes nothing unless the operator asks it to.**
//!
//! That is a decision, not an omission, and it is the opposite of what the
//! in-memory store does. [`TaskStoreConfig`](super::TaskStoreConfig) defaults
//! to a one-hour TTL and a 10,000-task cap, so the default in-process
//! deployment forgets a task an hour after it finishes. `SqliteTaskStore` and
//! `PostgresTaskStore` never read that config — they took a URL and nothing
//! else — so the durable deployment kept every task forever. Two opposite
//! behaviours, neither written down where an operator would look, and the
//! divergence was the actual defect: not that the table grows, but that
//! nothing said it would.
//!
//! Forgetting is right for a cache and wrong for a database. A library that
//! quietly deleted rows from an operator's `PostgreSQL` would be a far worse
//! surprise than one that grows, and "how long do we keep completed work" is a
//! question with legal answers, not just engineering ones — retention
//! schedules, audit obligations, and the customer's own contracts all have a
//! say. So the default stays "keep everything", and this module is the
//! mechanism for any other answer.
//!
//! # Using it
//!
//! `purge_expired` on each persistent store deletes terminal
//! tasks older than [`RetentionPolicy::terminal_max_age`], in batches, and
//! reports what it did. Call it from whatever already schedules work —
//! a cron, a Kubernetes `CronJob`, a `tokio::spawn` loop. It is deliberately
//! not wired to a timer inside the store: a sweep that fires on its own is a
//! sweep that fires during your traffic peak, and the store does not know when
//! that is.
//!
//! The example is gated on `sqlite` because the type it needs is: this crate
//! has no default features, so a doctest naming `SqliteTaskStore`
//! unconditionally fails to compile in every build that does not ask for a
//! backend — which is most of the CI matrix.
//!
//! ```no_run
//! # #[cfg(feature = "sqlite")]
//! # mod example {
//! use a2a_protocol_server::store::{RetentionPolicy, SqliteTaskStore};
//! use a2a_protocol_types::error::A2aResult;
//! use std::time::Duration;
//!
//! pub async fn sweep(store: &SqliteTaskStore) -> A2aResult<()> {
//!     let policy = RetentionPolicy::new(Duration::from_secs(30 * 24 * 3600));
//!     let report = store.purge_expired(&policy).await?;
//!     println!("purged {} task(s)", report.tasks_deleted);
//!     Ok(())
//! }
//! # }
//! ```
//!
//! # What it will not delete
//!
//! Only terminal tasks — `Completed`, `Failed`, `Canceled`, `Rejected`. A task
//! that is still `Working`, or parked in `InputRequired` waiting for a human,
//! is never eligible however old it is. The in-memory store does evict
//! non-terminal tasks as a last resort under capacity pressure, because RAM is
//! a hard bound; disk is not, and an unbounded-age `InputRequired` task is a
//! workflow waiting on someone, not a leak.

#[cfg(feature = "postgres")]
pub(crate) mod postgres;
#[cfg(feature = "sqlite")]
pub(crate) mod sqlite;

use std::time::Duration;

use a2a_protocol_types::task::TaskState;

/// The states a purge is allowed to delete.
///
/// Listed here rather than derived from `TaskState::ALL`, and that is a
/// packaging constraint rather than a preference. `cargo package` verifies the
/// server tarball against `a2a-protocol-types` **from crates.io** — the path
/// dependency is stripped, and the published 0.9.0 has no `ALL` — so
/// referencing it from library code fails the packaging gate on every PR until
/// a version bump makes the local copy the only candidate.
///
/// The guard therefore lives in the test below, which *does* see the local
/// crate: `cargo package` builds the library and not the tests, so a
/// `#[cfg(test)]` reference to `TaskState::ALL` costs nothing at packaging
/// time and still fails the moment this list stops agreeing with
/// [`TaskState::is_terminal`] across every variant the protocol defines.
const TERMINAL_STATES: [TaskState; 4] = [
    TaskState::Completed,
    TaskState::Failed,
    TaskState::Canceled,
    TaskState::Rejected,
];

/// The states a purge is allowed to delete.
#[must_use]
pub fn terminal_states() -> Vec<TaskState> {
    TERMINAL_STATES.to_vec()
}

/// How long an idempotency key is kept when nothing else is asked for.
///
/// One day. The number has to exceed the longest window in which a client
/// might still retry a send and expect a replay rather than a second
/// execution, and clients retry in seconds — `RetryPolicy`'s own schedule is
/// bounded in the low tens of seconds. A day is the figure the wider industry
/// settled on for the same trade, and it is far enough above any retry budget
/// this SDK ships that the choice is not delicate.
///
/// See [`RetentionPolicy::idempotency_key_max_age`] for what expiring a key
/// costs.
pub const DEFAULT_IDEMPOTENCY_KEY_MAX_AGE: Duration = Duration::from_secs(24 * 3600);

/// How long terminal tasks are kept, and how aggressively they are removed.
///
/// `#[non_exhaustive]`: build it with [`new`](Self::new) and the `with_*`
/// setters, which cover every field. `STABILITY.md` §4 lists the
/// configuration structs that carry this marking so a new option on any of
/// them is additive; this one was missed by the 0.12.0 conversion that
/// introduced the rule, and adding
/// [`idempotency_key_max_age`](Self::idempotency_key_max_age) is what found
/// it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RetentionPolicy {
    /// Terminal tasks whose last update is older than this are eligible.
    ///
    /// Measured against `updated_at`, which the store maintains, and evaluated
    /// by the database rather than the caller — an application clock that runs
    /// fast would otherwise delete work that is younger than it looks.
    pub terminal_max_age: Duration,

    /// Rows per `DELETE`. Default 1,000.
    ///
    /// The point of batching is the lock, not the throughput. One statement
    /// deleting a million rows holds locks and grows a transaction for as long
    /// as it takes; a thousand statements deleting a thousand rows each let
    /// every other query through in between.
    pub batch_size: u32,

    /// Idempotency keys older than this are deleted. `None` keeps them for
    /// ever, which is what every release before this one did.
    ///
    /// # Why a key has to expire at all
    ///
    /// A key is released when the send it guards fails, and otherwise it
    /// stays — deliberately, because it has to outlive the task it names or a
    /// late retry would execute a second time. Nothing else ever removed one.
    /// A busy deployment therefore accumulated a row per keyed send for the
    /// life of the database, and the index that makes the claim atomic grew
    /// with it.
    ///
    /// # What expiring one costs
    ///
    /// Exactly what the key was preventing: a retry arriving **after** the
    /// key expires re-executes the send. That is the trade every idempotency
    /// key has, and the reason the default is a full day rather than
    /// something tidier — it has to exceed the longest window in which a
    /// client might still retry, and clients retry in seconds.
    ///
    /// # It is never allowed below `terminal_max_age`
    ///
    /// A key expiring while the task it names is still retained is the bad
    /// case: the retry does not replay, it creates a *second* task alongside
    /// the first, and the caller ends up with two ids for one logical send.
    /// So the sweep uses
    /// [`effective_idempotency_key_max_age`](Self::effective_idempotency_key_max_age),
    /// which never returns less than
    /// [`terminal_max_age`](Self::terminal_max_age) — the invariant holds by
    /// construction rather than by the operator having read this paragraph.
    pub idempotency_key_max_age: Option<Duration>,

    /// Stop after this many batches, leaving the rest for the next call.
    /// `None` runs until nothing is left.
    ///
    /// Set it to bound how long one sweep can run when the first sweep after
    /// switching retention on has years of backlog to work through.
    pub max_batches: Option<u32>,
}

impl RetentionPolicy {
    /// A policy keeping terminal tasks for `terminal_max_age`, with default
    /// batching.
    #[must_use]
    pub const fn new(terminal_max_age: Duration) -> Self {
        Self {
            terminal_max_age,
            batch_size: 1_000,
            idempotency_key_max_age: Some(DEFAULT_IDEMPOTENCY_KEY_MAX_AGE),
            max_batches: None,
        }
    }

    /// Sets the rows-per-`DELETE` batch size. Zero is treated as one.
    #[must_use]
    pub const fn with_batch_size(mut self, batch_size: u32) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Sets how long an idempotency key is kept; `None` keeps them for ever.
    ///
    /// See [`idempotency_key_max_age`](Self::idempotency_key_max_age) for what
    /// expiring one costs and why the sweep will not honour a value below
    /// [`terminal_max_age`](Self::terminal_max_age).
    #[must_use]
    pub const fn with_idempotency_key_max_age(mut self, max_age: Option<Duration>) -> Self {
        self.idempotency_key_max_age = max_age;
        self
    }

    /// The key age the sweep actually uses: never below
    /// [`terminal_max_age`](Self::terminal_max_age).
    ///
    /// A key must outlive the task it names. Deleting one while its task is
    /// still retained turns the next retry into a second task rather than a
    /// replay, so a policy that asks for that is clamped rather than obeyed —
    /// the same shape as `effective_batch_size` — which is `pub(crate)` and
    /// feature-gated, so it cannot be linked from a public doc comment —
    /// where a nonsensical value is floored at the nearest sensible one
    /// instead of being honoured into a defect.
    #[must_use]
    pub fn effective_idempotency_key_max_age(&self) -> Option<Duration> {
        self.idempotency_key_max_age
            .map(|age| age.max(self.terminal_max_age))
    }

    /// Bounds how many batches a single sweep runs.
    #[must_use]
    pub const fn with_max_batches(mut self, max_batches: u32) -> Self {
        self.max_batches = Some(max_batches);
        self
    }

    /// The batch size actually used, never zero — a zero-size batch would
    /// delete nothing forever while reporting progress.
    // Feature-gated because its only callers are the two backends. Without
    // either, `-D warnings` makes it dead code -- and RUSTFLAGS applies to
    // this crate even when it is built as a dependency of something else,
    // which is why a policy type with no backend compiled broke fifteen CI
    // jobs that never mention retention.
    #[cfg(any(feature = "sqlite", feature = "postgres"))]
    #[must_use]
    pub(crate) const fn effective_batch_size(&self) -> u32 {
        if self.batch_size == 0 {
            1
        } else {
            self.batch_size
        }
    }
}

/// What one call to `purge_expired` did.
///
/// `#[non_exhaustive]`: read its fields, do not construct or exhaustively
/// destructure it. A sweep that learns to count something new should not be a
/// breaking change — 0.13.0's breaking list already carries one renamed field
/// of this struct.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PurgeReport {
    /// Task rows deleted.
    pub tasks_deleted: u64,
    /// Side-table rows this sweep had to reclaim itself: artifact-journal
    /// rows and event-log rows whose task was deleted.
    ///
    /// Normally **zero**, and that is the healthy reading: both tables have
    /// an `ON DELETE CASCADE`, so on a pool with `foreign_keys=ON` — which
    /// `SqliteTaskStore::new` sets — the rows go with the task and the sweep
    /// finds nothing left to do. A non-zero count means rows had outlived
    /// their task, which happens when `from_pool` was handed a pool without
    /// the pragma.
    ///
    /// # It is zero on `PostgreSQL` because nothing counts there
    ///
    /// Only the `SQLite` sweep runs anti-join deletes; the `PostgreSQL` one
    /// has no orphan statement at all, so this field is *structurally* zero on
    /// both Postgres stores rather than observed to be. That is a deliberate
    /// omission and not an oversight: `PostgreSQL` has no per-session
    /// equivalent of `foreign_keys=OFF`, so a declared `ON DELETE CASCADE`
    /// always fires and there is nothing for a sweep to find.
    ///
    /// The residual case it does **not** cover is a caller who handed
    /// `from_pool` a database in which `task_events` or `tenant_task_events`
    /// already existed without the foreign key — `CREATE TABLE IF NOT EXISTS`
    /// leaves such a table alone. Rows can then be stranded there and no
    /// Postgres sweep will reclaim them. Declare the tables from this crate's
    /// own DDL, or from its migration runner, and the case does not arise.
    ///
    /// Since 0.13 the `SQLite` sweep runs on every purge rather than only on
    /// one that deleted a task: a purge that fails part way strands rows that
    /// outlive it, and the sweep is what reclaims them.
    pub orphan_rows_deleted: u64,
    /// Idempotency keys deleted for being older than
    /// [`RetentionPolicy::idempotency_key_max_age`].
    ///
    /// Zero when that is `None`, and zero on a sweep that finds none expired.
    /// Unlike [`orphan_rows_deleted`](Self::orphan_rows_deleted) a non-zero
    /// count here is the healthy reading, not a warning: it is the sweep doing
    /// the job it was given.
    pub idempotency_keys_deleted: u64,

    /// Batches executed.
    pub batches: u32,
    /// `false` when [`RetentionPolicy::max_batches`] stopped the sweep with
    /// work still to do, so a caller can tell "nothing left" from "ran out of
    /// budget" instead of inferring it from a count.
    pub complete: bool,
}

/// The `state` column values a purge matches.
///
/// Built from [`TaskState`]'s own `Display`, not from string literals: the
/// column holds whatever `to_string()` produced at write time, and a purge
/// filtering on a hand-copied spelling would match nothing while looking
/// correct.
#[cfg(any(feature = "sqlite", feature = "postgres"))]
pub(crate) fn terminal_state_labels() -> Vec<String> {
    terminal_states().iter().map(TaskState::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_matches_is_terminal_for_every_variant() {
        // `TaskState::ALL` is only reachable here, in test code compiled
        // against the workspace copy of a2a-protocol-types. That is the point:
        // it gives this crate an enumeration of a `#[non_exhaustive]` foreign
        // enum without the library depending on an API the published version
        // does not have yet.
        let purgeable = terminal_states();
        for state in TaskState::ALL {
            assert_eq!(
                purgeable.contains(&state),
                state.is_terminal(),
                "{state} must be purgeable exactly when it is terminal"
            );
        }
        assert_eq!(
            purgeable.len(),
            TaskState::ALL.iter().filter(|s| s.is_terminal()).count(),
            "TERMINAL_STATES has drifted from is_terminal(); a new terminal \
             state would otherwise never be purged"
        );
    }

    #[test]
    #[cfg(any(feature = "sqlite", feature = "postgres"))]
    fn labels_are_the_stored_spellings() {
        let labels = terminal_state_labels();
        assert!(labels.contains(&"TASK_STATE_COMPLETED".to_string()));
        assert!(labels.contains(&"TASK_STATE_REJECTED".to_string()));
        assert_eq!(labels.len(), terminal_states().len());
        // The store writes `task.status.state.to_string()`; if that ever stops
        // agreeing with what a purge looks for, every sweep silently becomes a
        // no-op.
        assert_eq!(labels[0], TaskState::Completed.to_string());
    }

    #[test]
    #[cfg(any(feature = "sqlite", feature = "postgres"))]
    fn zero_batch_size_cannot_stall_a_sweep() {
        let policy = RetentionPolicy::new(Duration::from_secs(1)).with_batch_size(0);
        assert_eq!(
            policy.effective_batch_size(),
            1,
            "a zero batch size would delete nothing while looping forever"
        );
    }

    #[test]
    fn builders_compose() {
        let policy = RetentionPolicy::new(Duration::from_secs(60))
            .with_batch_size(50)
            .with_max_batches(3);
        assert_eq!(policy.terminal_max_age, Duration::from_secs(60));
        assert_eq!(policy.batch_size, 50);
        assert_eq!(policy.max_batches, Some(3));
    }

    #[test]
    fn report_defaults_to_nothing_done_but_complete() {
        let report = PurgeReport::default();
        assert_eq!(report.tasks_deleted, 0);
        assert!(!report.complete, "default must not claim a completed sweep");
    }
}

// ── The idempotency key TTL and its floor ────────────────────────────────────

#[cfg(test)]
mod key_age_tests {
    use super::{DEFAULT_IDEMPOTENCY_KEY_MAX_AGE, RetentionPolicy};
    use std::time::Duration;

    #[test]
    fn a_new_policy_expires_keys_after_a_day() {
        let policy = RetentionPolicy::new(Duration::from_secs(3600));
        assert_eq!(
            policy.idempotency_key_max_age,
            Some(DEFAULT_IDEMPOTENCY_KEY_MAX_AGE),
            "keys accumulated for ever before this default existed"
        );
        assert_eq!(DEFAULT_IDEMPOTENCY_KEY_MAX_AGE, Duration::from_secs(86_400));
    }

    /// The invariant the whole design rests on: a key outlives the task it
    /// names.
    ///
    /// A key expiring while its task is still retained does not produce a
    /// replay and does not produce a clear "that task is gone" — it produces
    /// a *second* task alongside the first, and the caller ends up with two
    /// ids for one logical send. So a policy asking for that is clamped
    /// rather than obeyed.
    #[test]
    fn a_key_age_below_the_task_age_is_raised_to_it() {
        let policy = RetentionPolicy::new(Duration::from_secs(30 * 24 * 3600))
            .with_idempotency_key_max_age(Some(Duration::from_secs(60)));
        assert_eq!(
            policy.effective_idempotency_key_max_age(),
            Some(Duration::from_secs(30 * 24 * 3600)),
            "a key must not be swept while the task it names is still kept"
        );
        assert_eq!(
            policy.idempotency_key_max_age,
            Some(Duration::from_secs(60)),
            "the field itself is untouched — the clamp is in the accessor, so \
             what the operator set stays readable"
        );
    }

    /// Counter-test: a key age above the task age is honoured exactly.
    ///
    /// Without it, an accessor that always returned `terminal_max_age` would
    /// satisfy the test above.
    #[test]
    fn a_key_age_above_the_task_age_is_used_as_given() {
        let policy = RetentionPolicy::new(Duration::from_secs(3600))
            .with_idempotency_key_max_age(Some(Duration::from_secs(7 * 24 * 3600)));
        assert_eq!(
            policy.effective_idempotency_key_max_age(),
            Some(Duration::from_secs(7 * 24 * 3600))
        );
    }

    #[test]
    fn opting_out_stays_opted_out() {
        let policy =
            RetentionPolicy::new(Duration::from_secs(3600)).with_idempotency_key_max_age(None);
        assert_eq!(
            policy.effective_idempotency_key_max_age(),
            None,
            "the clamp must not resurrect a sweep the operator turned off"
        );
    }
}
