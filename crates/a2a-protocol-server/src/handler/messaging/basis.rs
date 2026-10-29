// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The read a continuation's save is built from, and the log position that
//! save records (spec §3.5.2; [`AppliedPosition`](crate::store::AppliedPosition)).
//!
//! A continuation saves its turn's task with `save_appending_history`, which
//! replaces the stored artifacts and metadata with the ones the send read.
//! Another replica's turn may still be folding events into the task
//! (`tests/multi_replica.rs`: the single-writer refusal does not cross
//! replicas); an artifact it folded after that read is gone from the stored
//! task once the save lands. Until 2026-10-20 the save was made outside a
//! position scope and kept the position that fold had stored, so the store
//! said the task reflected an event it had just lost. A subscriber starting
//! from (task, position) on a replica with no live queue replays the log
//! after the position, and the event was in neither: missing from its
//! stream, with no `streamLagged` (`tests/stale_write_position`).
//!
//! So the save is built from the read the queue lease already makes to
//! number the turn (`admission::resume_after`), and records that read's
//! position, so the stored position claims no more than the task holds: a
//! subscriber replays from where the read was, and an event the save kept
//! after all (its history, say) is delivered twice rather than not at all.
//! The turn's first fold stores a position past the overwritten event
//! again; that the task lacks it is the lost update `store/applied.rs`
//! describes, which only a compare-and-set write would refuse.
//!
//! # Why lowering the stored position cannot number two events alike
//!
//! A new turn is numbered after the later of the log's end and the stored
//! position, because a failed append leaves the stored position past the log
//! (`store/applied.rs`). The position this save records is the lease read's,
//! and the turn is numbered after it. A position above it that the log
//! already held when the lease read it stays covered by the log's end. The
//! only positions the save can drop the stored one below are those that read
//! saw in neither place: issued by another replica's turn that is still
//! running. This turn's numbering, made from the same read, can already give
//! them to its own events, and its first fold lowers the stored position
//! below them in the same way. That is the concurrent-turns case, open and
//! pinned by `tests/multi_replica.rs`, not a new one.

use a2a_protocol_types::task::Task;

/// The task as the queue lease read it, and the log position it reflects.
#[derive(Debug)]
pub(super) struct LeaseRead {
    task: Task,
    position: u64,
}

impl LeaseRead {
    pub(super) const fn new(task: Task, position: u64) -> Self {
        Self { task, position }
    }

    /// The read taken under the claim, at `position`, for a continuation:
    /// the lease uses it rather than reading the row again (`claimed`).
    pub(super) fn under_claim(
        position: Option<u64>,
        stored: Option<&Task>,
        continues: bool,
    ) -> Option<Self> {
        position
            .filter(|_| continues)
            .zip(stored.cloned())
            .map(|(position, task)| Self::new(task, position))
    }

    pub(super) const fn position(&self) -> u64 {
        self.position
    }
}

/// `task`, a continuation's, with the artifacts and metadata of `read`, and
/// the position its save records; `task` as it is and no position for a new
/// task, or when the store reported no position.
///
/// The artifacts and metadata are the fields the save replaces from `task`;
/// its status is the turn's own and its history is appended to the stored
/// one, so those are kept.
///
/// The `bool` says the task returned is the whole row to store, so the save
/// need not read the row to append to its history, and the turn's event
/// consumer need not read it back: a new task, which is saved as it is; or a
/// continuation built from `read` while the turn holds its claim (`claimed`),
/// whose history is `read`'s with this turn's message appended and trimmed.
/// That is exactly what `TaskStore::save_appending_history` would store,
/// because under the claim no other turn writes the row between the read and
/// the save. Each read so saved was one round trip to the database on
/// PostgreSQL.
///
/// Without a claim, nothing stops another replica's turn from writing the
/// row after `read` (`tests/stale_write_position`), and a whole row would
/// overwrite what it wrote to the history, which the append keeps: the
/// history is left to the append.
pub(super) fn based_on(
    mut task: Task,
    read: Option<LeaseRead>,
    continues: bool,
    claimed: bool,
) -> (Task, Option<u64>, bool) {
    if !continues {
        return (task, None, true);
    }
    match read {
        Some(read) if read.task.id == task.id => {
            task.artifacts = read.task.artifacts;
            task.metadata = read.task.metadata;
            if !claimed {
                return (task, Some(read.position), false);
            }
            let mut history = read.task.history.unwrap_or_default();
            history.extend(task.history.take().unwrap_or_default());
            super::decisions::TASK_HISTORY_LIMIT.trim(&mut history);
            task.history = Some(history);
            (task, Some(read.position), true)
        }
        _ => (task, None, false),
    }
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::artifact::Artifact;
    use a2a_protocol_types::message::Part;
    use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

    use super::*;

    fn task(id: &str, artifact: &str) -> Task {
        Task {
            id: TaskId::new(id),
            context_id: ContextId::new("ctx"),
            status: TaskStatus::new(TaskState::Submitted),
            history: None,
            artifacts: Some(vec![Artifact::new(artifact, vec![Part::text("x")])]),
            metadata: Some(serde_json::json!({ "from": artifact })),
        }
    }

    #[test]
    fn a_continuation_is_saved_as_the_lease_read_it_at_that_position() {
        let mut read_task = task("t", "read");
        read_task.status = TaskStatus::new(TaskState::Working);
        let read = || Some(LeaseRead::new(read_task.clone(), 7));
        let (based, at, whole) = based_on(task("t", "sent"), read(), true, true);
        assert_eq!(at, Some(7));
        assert!(
            whole,
            "built from the read under the claim, it is the row to store"
        );
        assert_eq!(based.artifacts, task("t", "read").artifacts);
        assert_eq!(based.metadata, task("t", "read").metadata);
        assert_eq!(based.status.state, TaskState::Submitted, "the turn's own");

        let mut sent = task("t", "sent");
        sent.history = Some(Vec::new());
        let (unclaimed, at, whole) = based_on(sent, read(), true, false);
        assert_eq!(at, Some(7));
        assert!(!whole, "with no claim another turn may write the history");
        assert_eq!(unclaimed.artifacts, task("t", "read").artifacts);
        assert_eq!(unclaimed.history, Some(Vec::new()), "left to the append");
    }

    #[test]
    fn a_new_task_or_no_position_is_saved_as_it_is_outside_a_scope() {
        let sent = task("t", "sent");
        let read = || Some(LeaseRead::new(task("t", "read"), 7));
        assert_eq!(
            based_on(sent.clone(), read(), false, false),
            (sent.clone(), None, true),
            "a new task is the whole row"
        );
        assert_eq!(
            based_on(sent.clone(), None, true, true),
            (sent.clone(), None, false),
            "a continuation with no read appends to what is stored"
        );
        let other = Some(LeaseRead::new(task("other", "read"), 7));
        assert_eq!(
            based_on(sent.clone(), other, true, true),
            (sent, None, false)
        );
    }

    /// A continuation saved whole holds the history `save_appending_history`
    /// would have stored from the same row: the read's, then this turn's
    /// message, trimmed to the same limit.
    #[tokio::test]
    async fn a_whole_continuation_holds_what_the_append_would_have_stored() {
        use a2a_protocol_types::message::{Message, MessageId, MessageRole};

        use crate::store::{InMemoryTaskStore, TaskStore};

        let message = |text: &str| {
            Message::new(
                MessageId::new(text),
                MessageRole::User,
                vec![Part::text(text)],
            )
        };
        let mut stored = task("t", "read");
        stored.history = Some(
            (0..super::super::decisions::MAX_TASK_HISTORY_MESSAGES + 3)
                .map(|i| message(&format!("old-{i}")))
                .collect(),
        );
        let mut sent = task("t", "sent");
        sent.history = Some(vec![message("new")]);

        let appended = InMemoryTaskStore::new();
        appended.save(&stored).await.unwrap();
        appended
            .save_appending_history(
                &based_on(sent.clone(), None, true, true).0,
                &[message("new")],
                super::super::decisions::TASK_HISTORY_LIMIT,
            )
            .await
            .unwrap();
        let (whole, _, is_whole) = based_on(sent, Some(LeaseRead::new(stored, 1)), true, true);
        assert!(is_whole);
        let via_append = appended.get(&TaskId::new("t")).await.unwrap().unwrap();
        assert_eq!(whole.history, via_append.history);
    }
}
