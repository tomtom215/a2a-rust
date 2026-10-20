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
pub(super) fn based_on(
    mut task: Task,
    read: Option<LeaseRead>,
    continues: bool,
) -> (Task, Option<u64>) {
    match read {
        Some(read) if continues && read.task.id == task.id => {
            task.artifacts = read.task.artifacts;
            task.metadata = read.task.metadata;
            (task, Some(read.position))
        }
        _ => (task, None),
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
        let (based, at) = based_on(task("t", "sent"), Some(LeaseRead::new(read_task, 7)), true);
        assert_eq!(at, Some(7));
        assert_eq!(based.artifacts, task("t", "read").artifacts);
        assert_eq!(based.metadata, task("t", "read").metadata);
        assert_eq!(based.status.state, TaskState::Submitted, "the turn's own");
    }

    #[test]
    fn a_new_task_or_no_position_is_saved_as_it_is_outside_a_scope() {
        let sent = task("t", "sent");
        let read = || Some(LeaseRead::new(task("t", "read"), 7));
        assert_eq!(based_on(sent.clone(), read(), false), (sent.clone(), None));
        assert_eq!(based_on(sent.clone(), None, true), (sent.clone(), None));
        let other = Some(LeaseRead::new(task("other", "read"), 7));
        assert_eq!(based_on(sent.clone(), other, true), (sent, None));
    }
}
