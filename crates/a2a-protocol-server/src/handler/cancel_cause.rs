// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Writes who stopped a task on its `Canceled` status (COR-04, E03 R3).
//!
//! The server knows the cause from the turn ([`ExecutorTurn::cancel_cause`]):
//! a `CancelTask`, an operator's halt, or a lease that lapsed. Every
//! `Canceled` a turn's writer passes on carries it, whoever wrote the status:
//! the default cancel hook, an executor's own hook, or the writer after a
//! hook that wrote none. An executor's own status message keeps its text and
//! gains the cause; a status with no message is given one that says it.

use a2a_protocol_types::cancel_cause::{CancelCause, set_cause};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use super::turn::ExecutorTurn;

/// Writes `cause` on `event` when it puts its task in `Canceled`.
pub fn stamp(event: &mut StreamResponse, cause: Option<CancelCause>) {
    match event {
        StreamResponse::Task(t) => stamp_status(&mut t.status, &t.id, &t.context_id, cause),
        StreamResponse::StatusUpdate(u) => {
            stamp_status(&mut u.status, &u.task_id, &u.context_id, cause);
        }
        _ => {}
    }
}

/// Writes `cause` on `status` when it is `Canceled`.
pub fn stamp_status(
    status: &mut TaskStatus,
    task_id: &TaskId,
    context_id: &ContextId,
    cause: Option<CancelCause>,
) {
    let Some(cause) = cause else { return };
    if status.state != TaskState::Canceled {
        return;
    }
    let message = status.message.get_or_insert_with(|| {
        Message::new(
            MessageId::new(uuid::Uuid::new_v4().to_string()),
            MessageRole::Agent,
            vec![Part::text(says(cause))],
        )
        .with_task_id(task_id.clone())
        .with_context_id(context_id.clone())
    });
    set_cause(message, cause);
}

/// The status message a cause is given when the status carried none.
const fn says(cause: CancelCause) -> &'static str {
    match cause {
        CancelCause::Halted => "canceled: an operator halted this work",
        CancelCause::LeaseLapsed => "canceled: the task's lease was not renewed in time",
        // `CancelCause` is non-exhaustive in another crate.
        _ => "canceled: a caller asked for it",
    }
}

impl ExecutorTurn {
    /// Who stopped this turn, if anyone did: a lapsed lease (which stops the
    /// turn as a halt does, so it is asked first), a halt, or a `CancelTask`.
    pub(crate) fn cancel_cause(&self) -> Option<CancelCause> {
        if self.is_lease_lapsed() {
            Some(CancelCause::LeaseLapsed)
        } else if self.is_halted() {
            Some(CancelCause::Halted)
        } else if self.is_cancel_requested() {
            Some(CancelCause::CancelRequested)
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "cancel_cause_tests.rs"]
pub mod tests;
