// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A reviewer's narrow way into a requester's task (adversarial review A4,
//! 2026-11-12). The gate's side is [`ApprovalGate::with_review_grants`]; this is
//! the handler's: scoping the call to the requester's partition, and holding it
//! to what a reviewer is there to do.
//!
//! [`ApprovalGate::with_review_grants`]: crate::approval::ApprovalGate::with_review_grants

use a2a_protocol_types::approval::{ApprovalDecision, ApprovalRequest};
use a2a_protocol_types::error::A2aError;
use a2a_protocol_types::message::Message;
use a2a_protocol_types::task::{Task, TaskState};

use super::RequestHandler;
use crate::approval::REVIEW_FOR_HEADER;
use crate::call_context::CallContext;
use crate::error::{ServerError, ServerResult};

/// The methods a review grant opens, and no others.
const REVIEW_METHODS: [&str; 3] = ["GetTask", "SendMessage", "SendStreamingMessage"];

fn refused(why: &str) -> ServerError {
    ServerError::Protocol(A2aError::permission_denied(why))
}

impl RequestHandler {
    /// The requester a reviewer's call is scoped to: `None` for a call that
    /// names none.
    ///
    /// # Errors
    ///
    /// `PermissionDenied` when the call names a requester and is not one a review
    /// grant covers: the gate grants none, the caller is not an approver the
    /// grant names, the method is not one it opens, or the header is empty.
    pub(crate) fn review_scope(&self, call: &CallContext) -> ServerResult<Option<String>> {
        let Some(requester) = call.http_headers().get(REVIEW_FOR_HEADER) else {
            return Ok(None);
        };
        let held = self
            .approval_gate
            .as_ref()
            .zip(call.caller_identity())
            .is_some_and(|(gate, caller)| gate.grants_review_to(caller));
        if held && REVIEW_METHODS.contains(&call.method()) && !requester.is_empty() {
            Ok(Some(requester.clone()))
        } else {
            Err(refused(
                "this caller holds no review grant that covers this call",
            ))
        }
    }

    /// Whether `call` is a reviewer's, scoped to a requester's partition. Only
    /// meaningful after [`review_scope`](Self::review_scope) has admitted it.
    fn is_review(&self, call: &CallContext) -> bool {
        matches!(self.review_scope(call), Ok(Some(_)))
    }

    /// The requester a call that has been admitted is a review of, for the
    /// audit record.
    #[cfg(feature = "audit")]
    pub(crate) fn reviewed_requester(&self, call: &CallContext) -> Option<String> {
        self.review_scope(call).ok().flatten()
    }

    /// A reviewer may send an approval decision and nothing else: a message
    /// that is not one would start, steer or continue the requester's task.
    ///
    /// # Errors
    ///
    /// `PermissionDenied` for any other message from a reviewer.
    pub(crate) fn ensure_review_answers_an_approval(
        &self,
        call: &CallContext,
        message: &Message,
    ) -> ServerResult<()> {
        if !self.is_review(call) || ApprovalDecision::read(message)?.is_some() {
            return Ok(());
        }
        Err(refused(
            "a review grant lets a reviewer answer an approval, and this message is not one",
        ))
    }

    /// A reviewer may read a task only while it waits for a decision.
    ///
    /// # Errors
    ///
    /// `TaskNotFound` for any other task, as for a task that is not there.
    pub(crate) fn ensure_review_sees_a_waiting_task(
        &self,
        call: &CallContext,
        task: &Task,
    ) -> ServerResult<()> {
        let waiting = task.status.state == TaskState::InputRequired
            && task
                .status
                .message
                .as_ref()
                .is_some_and(|m| matches!(ApprovalRequest::read(m), Ok(Some(_))));
        if !self.is_review(call) || waiting {
            Ok(())
        } else {
            Err(ServerError::TaskNotFound(task.id.clone()))
        }
    }
}
