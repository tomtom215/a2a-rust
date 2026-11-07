// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests of the retention policy and report; the parked-task sweeps are in
//! [`parked`].

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

// ── The idempotency key TTL and its floor ────────────────────────────────────

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
        assert_eq!(DEFAULT_IDEMPOTENCY_KEY_MAX_AGE, Duration::from_hours(24));
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
        let policy = RetentionPolicy::new(Duration::from_hours(30 * 24))
            .with_idempotency_key_max_age(Some(Duration::from_secs(60)));
        assert_eq!(
            policy.effective_idempotency_key_max_age(),
            Some(Duration::from_hours(30 * 24)),
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
            .with_idempotency_key_max_age(Some(Duration::from_hours(7 * 24)));
        assert_eq!(
            policy.effective_idempotency_key_max_age(),
            Some(Duration::from_hours(7 * 24))
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

#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod parked;
