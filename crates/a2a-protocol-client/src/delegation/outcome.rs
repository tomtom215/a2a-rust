// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`Outcome`] and [`Delegated`]: what became of a delegated task.

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
    /// The child reached `canceled`, whoever asked for it.
    Canceled(TaskStatus),
    /// The child is waiting for the parent: `input-required` or
    /// `auth-required`. It was not cancelled; continue it by sending a
    /// message with its task id, or cancel it.
    Interrupted(TaskStatus),
    /// The agent answered with a message and created no task.
    Message(Message),
    /// The child's stream failed or ended before the child settled. The
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
