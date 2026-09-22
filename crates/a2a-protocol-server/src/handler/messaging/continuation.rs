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

/// What [`resolve_task_id`](super::MessageHandler::resolve_task_id) decided.
pub(super) struct Resolution {
    /// The id this send will use.
    pub(super) id: TaskId,
    /// The task this send continues, set **only** when that is not the one
    /// `find_task_by_context` returned.
    ///
    /// `None` is the common case and means the caller's own `stored_task` is
    /// still the right view of the past: it covers both a fresh task and a
    /// continuation of the context's canonical one. The caller substitutes
    /// this when it is `Some`, so every path that existed before this type
    /// behaves exactly as it did.
    ///
    /// It has to be carried rather than re-derived, because
    /// `create::build_initial_task` keeps the stored history and artifacts
    /// only when the stored task's id equals the resolved one. Handing it a
    /// task whose id does not match silently starts the continuation from an
    /// empty history.
    pub(super) continues: Option<Task>,
}

impl Resolution {
    /// The resolved id, with the caller's `stored_task` left in place.
    pub(super) const fn fresh(id: TaskId) -> Self {
        Self {
            id,
            continues: None,
        }
    }

    /// The resolved id, continuing a task other than the canonical one.
    pub(super) const fn continues(id: TaskId, task: Task) -> Self {
        Self {
            id,
            continues: Some(task),
        }
    }
}

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

    /// Determines which task this send belongs to: the client-provided one
    /// when it names a live task in this context, otherwise a fresh one
    /// (A2A spec §3.4.3).
    ///
    /// Must be called under the per-context lock, with `stored_task` the
    /// task found for the context while holding it.
    ///
    /// # Errors
    ///
    /// * [`ServerError::InvalidParams`] when the message names a task that
    ///   exists under a different context — the one mismatch §3.4.3 requires
    ///   an agent to reject.
    /// * [`ServerError::UnsupportedOperation`] when the named task is in a
    ///   terminal state (SPEC CORE-SEND-002).
    /// * [`ServerError::TaskNotFound`] when the named task does not exist at
    ///   all (SPEC §3.4.2).
    pub(super) async fn resolve_task_id(
        &self,
        message: &Message,
        stored_task: Option<&Task>,
    ) -> ServerResult<Resolution> {
        let Some(ref msg_task_id) = message.task_id else {
            // §3.4.3: "Clients MAY use contextId without taskId to start a new
            // task within an existing conversation context." So this forks
            // unconditionally, and deliberately: it does not consult
            // `stored_task` at all, terminal or not.
            return Ok(Resolution::fresh(TaskId::new(
                uuid::Uuid::new_v4().to_string(),
            )));
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
            // Not the task `find_task_by_context` returned — which is only
            // ever *one* task, the most recently updated live one. That does
            // not make this a mismatch. §3.4.1: "A contextId logically groups
            // multiple Task objects"; §3.4.3: "Clients MAY use taskId (with or
            // without contextId) to continue or refine a specific task", and
            // the sole rejection it mandates is a contextId differing from the
            // referenced task's. So the question to ask is about the named
            // task's context, not about its identity with the canonical one.
            //
            // This used to reject outright, which locked every participant out
            // of a channel as soon as one of them posted without a taskId: the
            // fork became canonical and the original, still live and still in
            // the same context, became unaddressable. Finding 2 of
            // `docs/swarm-scale-findings.md` measured that.
            let Some(named) = self.task_store.get(msg_task_id).await? else {
                return Err(ServerError::TaskNotFound(msg_task_id.clone()));
            };
            if named.context_id != stored.context_id {
                return Err(ServerError::InvalidParams(
                    "task_id exists but belongs to a different context".into(),
                ));
            }
            if named.status.state.is_terminal() {
                return Err(ServerError::UnsupportedOperation(format!(
                    "task {} is in terminal state '{}' and cannot accept new messages",
                    named.id, named.status.state
                )));
            }
            return Ok(Resolution::continues(msg_task_id.clone(), named));
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
        Ok(Resolution::fresh(msg_task_id.clone()))
    }
}
