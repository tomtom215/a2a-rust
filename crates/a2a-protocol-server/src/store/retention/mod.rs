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
//! [`purge_expired`](super::SqliteTaskStore::purge_expired) deletes terminal
//! tasks older than [`RetentionPolicy::terminal_max_age`], in batches, and
//! reports what it did. Call it from whatever already schedules work —
//! a cron, a Kubernetes `CronJob`, a `tokio::spawn` loop. It is deliberately
//! not wired to a timer inside the store: a sweep that fires on its own is a
//! sweep that fires during your traffic peak, and the store does not know when
//! that is.
//!
//! ```no_run
//! # use a2a_protocol_server::store::{RetentionPolicy, SqliteTaskStore};
//! # use std::time::Duration;
//! # async fn example(store: &SqliteTaskStore) -> Result<(), Box<dyn std::error::Error>> {
//! let policy = RetentionPolicy::new(Duration::from_secs(30 * 24 * 3600));
//! let report = store.purge_expired(&policy).await?;
//! println!("purged {} task(s)", report.tasks_deleted);
//! # Ok(())
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

/// The states a purge is allowed to delete, derived from
/// [`TaskState::is_terminal`] over [`TaskState::ALL`].
///
/// Not a hand-written list: `TaskState` is `#[non_exhaustive]`, so this crate
/// cannot match over it exhaustively, and a literal array here would keep
/// compiling — and keep looking right — after a new terminal state was added
/// that it did not mention. `ALL` carries that guard in the crate that can
/// enforce it.
#[must_use]
pub fn terminal_states() -> Vec<TaskState> {
    TaskState::ALL
        .iter()
        .copied()
        .filter(|state| state.is_terminal())
        .collect()
}

/// How long terminal tasks are kept, and how aggressively they are removed.
#[derive(Debug, Clone)]
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
            max_batches: None,
        }
    }

    /// Sets the rows-per-`DELETE` batch size. Zero is treated as one.
    #[must_use]
    pub const fn with_batch_size(mut self, batch_size: u32) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Bounds how many batches a single sweep runs.
    #[must_use]
    pub const fn with_max_batches(mut self, max_batches: u32) -> Self {
        self.max_batches = Some(max_batches);
        self
    }

    /// The batch size actually used, never zero — a zero-size batch would
    /// delete nothing forever while reporting progress.
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// Task rows deleted.
    pub tasks_deleted: u64,
    /// Artifact-journal rows this sweep had to reclaim itself.
    ///
    /// Normally **zero**, and that is the healthy reading: the journal has an
    /// `ON DELETE CASCADE`, so on a pool with `foreign_keys=ON` — which
    /// `SqliteTaskStore::new` sets — the rows go with the task and the sweep
    /// finds nothing left to do. A non-zero count means rows had outlived
    /// their task, which happens when `from_pool` was handed a pool without
    /// the pragma. Always zero on `PostgreSQL`, which has no journal table.
    pub journal_orphans_deleted: u64,
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
pub(crate) fn terminal_state_labels() -> Vec<String> {
    terminal_states().iter().map(TaskState::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_matches_is_terminal_for_every_variant() {
        // The set is derived, so this checks the derivation rather than a
        // hand-copied list: every state the protocol defines is purgeable
        // exactly when it is terminal. `TaskState::ALL` is what makes "every
        // state" a claim this can actually make — see its guard in
        // a2a-protocol-types.
        let purgeable = terminal_states();
        for state in TaskState::ALL {
            assert_eq!(
                purgeable.contains(&state),
                state.is_terminal(),
                "{state} must be purgeable exactly when it is terminal"
            );
        }
        assert_eq!(purgeable.len(), 4, "Completed, Failed, Canceled, Rejected");
    }

    #[test]
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
