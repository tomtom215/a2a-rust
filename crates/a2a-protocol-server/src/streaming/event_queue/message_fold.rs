// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Carrying a message the agent writes during a task in a status update.
//!
//! Spec §3.1.2 allows a stream of exactly one `Message`, or a stream that
//! begins with the `Task` and continues with status and artifact updates.
//! A `Message` an executor writes once its task stream has begun fits
//! neither, and until 2026-10-23 it was forwarded as it was. Its conforming
//! carrier is `TaskStatus.message`, so the writer sends it as a status
//! update that keeps the task's state and carries the message.
//!
//! The fold happens here, once, before the event is given its position, so
//! every reader of that position (the send stream, a `SubscribeToTask`
//! stream, a resumed one replaying the log) gets the same payload (§3.5.2).
//! A fold in each reader, tried first, gave a replayed message the task's
//! current state instead of the state it was written in.
//!
//! What is folded:
//!
//! * a `Message` written after the turn has written a `Task`, a status
//!   update or an artifact update, in the state the turn last wrote, or the
//!   state the task was in when the turn began
//!   ([`InMemoryQueueWriter::bind_state`](super::InMemoryQueueWriter::bind_state)).
//!
//! What is not:
//!
//! * a turn's first event, and a `Message` after only messages: that is the
//!   message-only exchange §3.1.2 allows, a direct reply;
//! * a `Message` after the turn wrote a terminal state: every stream has
//!   ended by then, and a terminal status update would be a second terminal
//!   event;
//! * a `Message` when no state is known (a writer the send path did not
//!   bind, and no status written yet).
//!
//! The folded update is marked ([`StreamEvent::folded_message`]), so the
//! processors that keep the task store treat it as the message it was:
//! appended to `history`, no state transition checked (a `Submitted` task
//! stays `Submitted`, which a transition may not do), and the stored status
//! message updated to it, as a client applying the update would.
//!
//! [`StreamEvent::folded_message`]: super::StreamEvent

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

/// What the fold knows of the turn so far; one per queue, shared by every
/// clone of its writer, read and changed under the writer's order lock.
#[derive(Debug, Default)]
pub(super) struct Turn {
    /// The turn has written a `Task`, a status update or an artifact update.
    task_stream: bool,
    /// The task's state as the turn last wrote it, or as the turn began.
    state: Option<TaskState>,
    /// The task and context the last status the turn wrote named, for a
    /// writer with no bound identity.
    ids: Option<(TaskId, ContextId)>,
}

impl Turn {
    /// The task's state as the turn begins; a state the turn already wrote
    /// is kept.
    pub(super) const fn begin_in(&mut self, state: TaskState) {
        if self.state.is_none() {
            self.state = Some(state);
        }
    }

    /// `event` as it is to be written, and whether it is a folded message.
    pub(super) fn fold(&self, event: StreamResponse) -> (StreamResponse, bool) {
        let StreamResponse::Message(message) = event else {
            return (event, false);
        };
        match self.state {
            Some(state) if self.task_stream && !state.is_terminal() => {
                // Empty ids are filled from the writer's binding
                // (`check_identity`); the last status's are for a writer
                // without one.
                let (task_id, context_id) = self
                    .ids
                    .clone()
                    .unwrap_or_else(|| (TaskId::new(""), ContextId::new("")));
                let update = TaskStatusUpdateEvent {
                    task_id,
                    context_id,
                    status: TaskStatus {
                        state,
                        message: Some(message),
                        timestamp: None,
                    },
                    metadata: None,
                };
                (StreamResponse::StatusUpdate(update), true)
            }
            _ => (StreamResponse::Message(message), false),
        }
    }

    /// Takes in what a written event said, once the write has succeeded: a
    /// write the queue failed reached no reader and no store, so the next
    /// message is not carried in a state nobody saw.
    pub(super) fn note(&mut self, note: Note) {
        match note {
            Note::Nothing => {}
            Note::Artifact => self.task_stream = true,
            Note::Status(state, task_id, context_id) => {
                self.task_stream = true;
                self.state = Some(state);
                self.ids = Some((task_id, context_id));
            }
        }
    }
}

/// What an event tells the fold, read before the write and applied
/// ([`Turn::note`]) once it has succeeded.
pub(super) enum Note {
    /// A message: the turn is not a task stream because of it.
    Nothing,
    /// An artifact update: the turn is a task stream.
    Artifact,
    /// A status update or a task snapshot, and the state and ids it names.
    Status(TaskState, TaskId, ContextId),
}

impl Note {
    pub(super) fn of(event: &StreamResponse) -> Self {
        match event {
            StreamResponse::StatusUpdate(u) => {
                Self::Status(u.status.state, u.task_id.clone(), u.context_id.clone())
            }
            StreamResponse::Task(t) => {
                Self::Status(t.status.state, t.id.clone(), t.context_id.clone())
            }
            StreamResponse::ArtifactUpdate(_) => Self::Artifact,
            _ => Self::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::artifact::{Artifact, ArtifactId};
    use a2a_protocol_types::events::TaskArtifactUpdateEvent;
    use a2a_protocol_types::message::{Message, MessageId, Part};

    use super::*;

    fn message() -> StreamResponse {
        StreamResponse::Message(Message::agent_text("m", "hello"))
    }

    fn status(state: TaskState) -> StreamResponse {
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new("t"),
            context_id: ContextId::new("c"),
            status: TaskStatus::new(state),
            metadata: None,
        })
    }

    fn artifact() -> StreamResponse {
        StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            task_id: TaskId::new("t"),
            context_id: ContextId::new("c"),
            artifact: Artifact::new(ArtifactId::new("a"), vec![Part::text("x")]),
            append: None,
            last_chunk: None,
            metadata: None,
        })
    }

    /// The status a folded message was given, or `None` if it was not.
    fn folded(turn: &Turn) -> Option<TaskStatusUpdateEvent> {
        match turn.fold(message()) {
            (StreamResponse::StatusUpdate(u), true) => Some(u),
            (StreamResponse::Message(_), false) => None,
            other => panic!("unexpected fold: {other:?}"),
        }
    }

    #[test]
    fn a_first_message_is_a_direct_reply() {
        let mut turn = Turn::default();
        turn.begin_in(TaskState::Submitted);
        assert!(folded(&turn).is_none());
        turn.note(Note::of(&message()));
        assert!(folded(&turn).is_none(), "after only messages");
    }

    #[test]
    fn a_message_after_a_status_keeps_its_state_and_ids() {
        let mut turn = Turn::default();
        turn.begin_in(TaskState::Submitted);
        turn.note(Note::of(&status(TaskState::InputRequired)));
        let update = folded(&turn).expect("folded");
        assert_eq!(update.status.state, TaskState::InputRequired);
        assert_eq!(update.task_id, TaskId::new("t"));
        assert_eq!(update.context_id, ContextId::new("c"));
        assert_eq!(
            update.status.message.expect("message").id,
            MessageId::new("m")
        );
    }

    #[test]
    fn a_message_after_an_artifact_takes_the_state_the_turn_began_in() {
        let mut turn = Turn::default();
        turn.begin_in(TaskState::Working);
        turn.note(Note::of(&artifact()));
        assert_eq!(
            folded(&turn).expect("folded").status.state,
            TaskState::Working
        );
    }

    #[test]
    fn a_message_after_a_terminal_state_is_not_folded() {
        let mut turn = Turn::default();
        turn.begin_in(TaskState::Working);
        turn.note(Note::of(&status(TaskState::Completed)));
        assert!(folded(&turn).is_none());
    }

    #[test]
    fn with_no_known_state_nothing_is_folded() {
        let mut turn = Turn::default();
        turn.note(Note::of(&artifact()));
        assert!(folded(&turn).is_none());
    }

    #[test]
    fn a_state_the_turn_wrote_outlives_a_late_binding() {
        let mut turn = Turn::default();
        turn.note(Note::of(&status(TaskState::Working)));
        turn.begin_in(TaskState::Submitted);
        assert_eq!(
            folded(&turn).expect("folded").status.state,
            TaskState::Working
        );
    }
}

/// The fold as the queue's writer drives it.
#[cfg(test)]
mod writer_tests {
    use a2a_protocol_types::artifact::Artifact;
    use a2a_protocol_types::events::{
        StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent,
    };
    use a2a_protocol_types::message::{Message, Part};
    use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

    use super::super::{EventQueueWriter as _, new_in_memory_queue, unshare};

    fn ids() -> (TaskId, ContextId) {
        (TaskId::new("t"), ContextId::new("c"))
    }

    /// A status the queue failed to write reached no reader and no store,
    /// so a later message is carried in the state the turn began in, not
    /// in the state of the failed write.
    #[tokio::test]
    async fn a_failed_write_does_not_set_the_state_a_message_is_carried_in() {
        let (writer, reader) = new_in_memory_queue();
        let (task_id, context_id) = ids();
        writer.bind_identity(task_id.clone(), context_id.clone());
        writer.bind_state(TaskState::Submitted);
        // No reader and no persistence: the write fails after its position
        // is assigned ("no active receivers").
        drop(reader);
        let failed = writer
            .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: task_id.clone(),
                context_id: context_id.clone(),
                status: TaskStatus::new(TaskState::Working),
                metadata: None,
            }))
            .await;
        assert!(failed.is_err(), "{failed:?}");

        let mut rx = writer.raw_subscribe();
        writer
            .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                task_id,
                context_id,
                artifact: Artifact::new("a", vec![Part::text("x")]),
                append: None,
                last_chunk: None,
                metadata: None,
            }))
            .await
            .expect("artifact");
        writer
            .write(StreamResponse::Message(Message::agent_text("m", "hi")))
            .await
            .expect("message");
        let _artifact = rx.recv().await.expect("artifact");
        let carried = unshare(rx.recv().await.expect("message")).expect("ok");
        assert!(carried.folded_message);
        let StreamResponse::StatusUpdate(update) = carried.event else {
            panic!("expected a status update, got {:?}", carried.event);
        };
        assert_eq!(update.status.state, TaskState::Submitted);
    }
}
