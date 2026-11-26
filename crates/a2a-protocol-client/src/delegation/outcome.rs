// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`Outcome`] and [`Delegated`]: what became of a delegated task.

use a2a_protocol_types::cancel_cause::{CancelCause, cause_of};
use a2a_protocol_types::failure::{FailureClass, class_of};
use a2a_protocol_types::{Message, TaskState, TaskStatus};

use crate::error::ClientError;

/// What became of a delegated task.
#[derive(Debug)]
#[non_exhaustive]
pub enum Outcome {
    /// The child completed.
    Completed(TaskStatus),
    /// The child failed or was rejected (`status.state` says which), with
    /// the class it declared, or [`FailureClass::Internal`] if it declared
    /// none.
    Failed {
        /// The child's final status.
        status: TaskStatus,
        /// Its failure class.
        class: FailureClass,
    },
    /// The child reached `canceled`; [`cancel_cause`](Self::cancel_cause)
    /// says who stopped it, when its server said.
    Canceled(TaskStatus),
    /// The child is waiting for the parent: `input-required` or
    /// `auth-required`. It was not cancelled; continue it by sending a
    /// message with its task id, or cancel it.
    Interrupted(TaskStatus),
    /// The agent answered with a message and created no task.
    Message(Message),
    /// The child's stream failed or ended before the child settled, and
    /// could not be resumed (see [`ResumePolicy`](super::ResumePolicy)). The
    /// child, if its id was known, was sent `CancelTask`.
    Lost(Option<ClientError>),
    /// A cancel was asked for, but the child's id was never learned within
    /// the wait, so no `CancelTask` could be sent. A child may be running.
    Unreachable,
    /// `CancelTask` was accepted, but the task it returned is not yet in a
    /// terminal state; the agent stops the child asynchronously.
    CancelRequested(TaskStatus),
    /// `CancelTask` was sent and refused or failed.
    CancelFailed(ClientError),
    /// The supervisor was shut down and handed the child over: the child was
    /// left running and its journal record left open, at the position the
    /// stream had reached, for the next supervisor to find with
    /// `DelegationJournal::open` and [`Delegation::reattach`](super::Delegation::reattach).
    /// See [`Delegation::wait_or_hand_off`](super::Delegation::wait_or_hand_off).
    HandedOff,
}

impl Outcome {
    pub(super) fn of(status: TaskStatus) -> Option<Self> {
        Some(match status.state {
            TaskState::Completed => Self::Completed(status),
            TaskState::Canceled => Self::Canceled(status),
            TaskState::Failed | TaskState::Rejected => {
                let class = status
                    .message
                    .as_ref()
                    .and_then(class_of)
                    .unwrap_or(FailureClass::Internal);
                Self::Failed { status, class }
            }
            s if s.is_interrupted() => Self::Interrupted(status),
            _ => return None,
        })
    }

    /// Who stopped a [`Canceled`](Self::Canceled) child: a `CancelTask`, an
    /// operator's halt or a lapsed lease (extension `cancel/v1`). `None` for
    /// any other outcome, and for a canceled status that names no cause this
    /// version knows, as one from another SDK does: never guessed.
    #[must_use]
    pub fn cancel_cause(&self) -> Option<CancelCause> {
        match self {
            Self::Canceled(status) => status.message.as_ref().and_then(cause_of),
            _ => None,
        }
    }
}

/// A delegation that has ended: the child's id, if it was learned, and its
/// outcome.
#[derive(Debug)]
#[non_exhaustive]
pub struct Delegated {
    /// The child's task id; `None` if no event named it.
    pub task_id: Option<String>,
    /// What became of it.
    pub outcome: Outcome,
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::cancel_cause::{CancelCause, set_cause};
    use a2a_protocol_types::{Message, TaskState, TaskStatus};

    use super::Outcome;

    /// COR-04: a canceled child says who stopped it when its server wrote a
    /// cause, and `None` when a peer wrote none.
    #[test]
    fn a_canceled_child_says_who_stopped_it_and_a_bare_one_says_nothing() {
        let bare = Outcome::of(TaskStatus::new(TaskState::Canceled)).unwrap();
        assert!(matches!(bare, Outcome::Canceled(_)));
        assert_eq!(bare.cancel_cause(), None);
        let mut note = Message::agent_text("m", "halted");
        set_cause(&mut note, CancelCause::Halted);
        let halted =
            Outcome::of(TaskStatus::new(TaskState::Canceled).with_message(note.clone())).unwrap();
        assert_eq!(halted.cancel_cause(), Some(CancelCause::Halted));
        let failed = Outcome::of(TaskStatus::new(TaskState::Failed).with_message(note)).unwrap();
        assert_eq!(
            failed.cancel_cause(),
            None,
            "only a canceled child has a cause"
        );
    }
}
