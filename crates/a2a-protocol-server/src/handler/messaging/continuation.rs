// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which context and task a message belongs to: a fresh task, or a
//! continuation of a stored one.
//!
//! The context is resolved first and unlocked; the task id is resolved under
//! the per-context lock the caller holds, so the find + decide + save
//! sequence for one context cannot interleave with another send's.

use a2a_protocol_types::message::Message;
use a2a_protocol_types::task::{Task, TaskId};

use super::super::RequestHandler;
use crate::error::{ServerError, ServerResult};

impl RequestHandler {
    /// Resolves the context id from the message, per the proto
    /// `SendMessageRequest` definition.
    ///
    /// SPEC §3.4.3: "Agents MUST infer contextId from the task if only taskId
    /// is provided" — so a taskId-only continuation looks up the referenced
    /// task's context instead of being rejected. A message with neither id
    /// starts a fresh context.
    ///
    /// # Errors
    ///
    /// [`ServerError::TaskNotFound`] when only a `taskId` was supplied and no
    /// such task exists (SPEC §3.4.2: a client-supplied taskId MUST reference
    /// an existing task).
    pub(super) async fn resolve_context_id(&self, message: &Message) -> ServerResult<String> {
        if let Some(ref ctx) = message.context_id {
            Ok(ctx.0.clone())
        } else if let Some(ref msg_task_id) = message.task_id {
            match self.task_store.get(msg_task_id).await? {
                Some(task) => Ok(task.context_id.0),
                None => Err(ServerError::TaskNotFound(msg_task_id.clone())),
            }
        } else {
            Ok(uuid::Uuid::new_v4().to_string())
        }
    }

    /// Determines the task id: the client-provided one when it matches the
    /// stored non-terminal task for this context (an input-required
    /// continuation, A2A spec §3.4.3), otherwise a fresh one.
    ///
    /// Must be called under the per-context lock, with `stored_task` the
    /// task found for the context while holding it.
    ///
    /// # Errors
    ///
    /// * [`ServerError::InvalidParams`] when the message names a task other
    ///   than the one stored for its context, or a task that exists under a
    ///   different context.
    /// * [`ServerError::UnsupportedOperation`] when the named task is in a
    ///   terminal state (SPEC CORE-SEND-002).
    /// * [`ServerError::TaskNotFound`] when the named task does not exist at
    ///   all (SPEC §3.4.2).
    pub(super) async fn resolve_task_id(
        &self,
        message: &Message,
        stored_task: Option<&Task>,
    ) -> ServerResult<TaskId> {
        let Some(ref msg_task_id) = message.task_id else {
            // No explicit task_id from client. If the found stored task is
            // terminal, a new task will be created on this context — this is
            // allowed (new conversation round on same context).
            return Ok(TaskId::new(uuid::Uuid::new_v4().to_string()));
        };
        let Some(stored) = stored_task else {
            // SPEC §3.4.2: When a client includes a taskId in a Message, it
            // MUST reference an existing task. Return TaskNotFound if the
            // task does not exist at all (not just absent from this context).
            let exists = self.task_store.get(msg_task_id).await?.is_some();
            if !exists {
                return Err(ServerError::TaskNotFound(msg_task_id.clone()));
            }
            // Task exists but under a different context — this is a mismatch.
            return Err(ServerError::InvalidParams(
                "task_id exists but belongs to a different context".into(),
            ));
        };
        if msg_task_id != &stored.id {
            return Err(ServerError::InvalidParams(
                "message task_id does not match task found for context".into(),
            ));
        }
        // SPEC CORE-SEND-002: Reject messages explicitly targeting a task in
        // terminal state. Tasks in Completed, Failed, Canceled, or Rejected
        // state cannot accept further messages.
        if stored.status.state.is_terminal() {
            return Err(ServerError::UnsupportedOperation(format!(
                "task {} is in terminal state '{}' and cannot accept new messages",
                stored.id, stored.status.state
            )));
        }
        // Reuse the existing task_id for non-terminal continuations.
        Ok(msg_task_id.clone())
    }
}
