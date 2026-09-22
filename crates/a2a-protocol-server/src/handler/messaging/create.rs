// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Building and committing the task: the initial `Task`, the executor's
//! `RequestContext`, the store write, and the inline push config — with the
//! rollback that keeps a failed commit from leaking the queue and token
//! admitted for it.

use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::SendMessageConfiguration;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use super::super::RequestHandler;
use super::decisions::MAX_TASK_HISTORY_MESSAGES;
use crate::error::ServerResult;
use crate::request_context::RequestContext;

/// The task as it is first saved: `Submitted`, with the incoming message
/// appended to the (capped) history.
///
/// Carried-forward state is scoped to the task it belongs to, which is not
/// the same thing as the context it belongs to. `resolve_task_id` has
/// already decided which of the two this send is: when it reuses the stored
/// task's id — an `InputRequired` continuation, A2A spec §3.4.3 — that
/// task's own history, artifacts and metadata come forward and only the
/// status returns to Submitted for the new turn; when it mints a fresh id
/// instead, this is a *new* task on an existing context and starts clean.
/// The stored task found for the context is then a different, usually
/// finished task, and its state is not this one's.
///
/// `a2a.proto` scopes all three fields to the task: artifacts are "a set of
/// output artifacts for a `Task`", history "the history of interactions
/// from a `Task`", metadata "custom metadata about a task". None of them is
/// a context-level aggregate. Keying the carry-forward on "a task exists
/// for this context" rather than "it is this task" made every round after
/// the first return the whole context's accumulated artifacts, growing by
/// one each turn ([#130]).
///
/// The incoming message is appended to `history` either way. `Task.history`
/// is the conversation record that `GetTask`'s `historyLength` truncates; a
/// multi-turn executor reads the *previous* task's turns from
/// [`RequestContext::stored_task`], which `build_request_context` passes it
/// separately and which this scoping does not touch.
///
/// [#130]: https://github.com/tomtom215/a2a-rust/issues/130
pub(super) fn build_initial_task(
    task_id: &TaskId,
    context_id: &str,
    stored_task: Option<&Task>,
    message: &Message,
) -> Task {
    // The task found for this context is this task's past only when the ids
    // match; `resolve_task_id` returns the stored id for a continuation and a
    // fresh uuid otherwise, so this equality is exactly that decision.
    let continuation = stored_task.filter(|s| s.id == *task_id);
    // Only the message this turn adds. The stored conversation is NOT copied
    // in: `persist_initial_task` hands this one message to
    // `TaskStore::save_appending_history`, which appends it to whatever the
    // store already holds. Copying it here was the second of the three
    // O(history) stages a send used to pay, and on a first turn — where the
    // store has nothing to append to and falls back to an insert — this one
    // message is already the whole correct history.
    let history = vec![message.clone()];
    Task {
        id: task_id.clone(),
        context_id: ContextId::new(context_id),
        status: TaskStatus::with_timestamp(TaskState::Submitted),
        history: Some(history),
        artifacts: continuation.and_then(|s| s.artifacts.clone()),
        metadata: continuation.and_then(|s| s.metadata.clone()),
    }
}

/// The executor's view of the request. Built before the task is saved so
/// its cancellation token can be registered atomically with the save.
pub(super) fn build_request_context(
    message: Message,
    task_id: TaskId,
    context_id: String,
    stored_task: Option<Task>,
    metadata: Option<serde_json::Value>,
    call_context: crate::call_context::CallContext,
) -> RequestContext {
    let mut ctx = RequestContext::new(message, task_id, context_id).with_call_context(call_context);
    if let Some(stored) = stored_task {
        ctx = ctx.with_stored_task(stored);
    }
    if let Some(meta) = metadata {
        ctx = ctx.with_metadata(meta);
    }
    ctx
}

impl RequestHandler {
    /// Persists the initial task. If the store refuses it, the queue and
    /// token admitted for it are released, so a store error leaks neither.
    ///
    /// # Errors
    ///
    /// The store's error, converted.
    pub(super) async fn persist_initial_task(&self, task: &Task) -> ServerResult<()> {
        // `task.history` holds only this turn's message (see
        // `build_initial_task`), and that is what gets appended. The store
        // keeps the conversation; the send path never holds it.
        let appended = task.history.clone().unwrap_or_default();
        if let Err(e) = self
            .task_store
            .save_appending_history(task, &appended, MAX_TASK_HISTORY_MESSAGES)
            .await
        {
            self.release_admission(&task.id).await;
            return Err(e.into());
        }
        Ok(())
    }

    /// Registers a push notification config carried inline on a `SendMessage`.
    ///
    /// The schema is explicit that this is how a client subscribes at send
    /// time: *"Task id should be empty when sending this configuration in a
    /// `SendMessage` request"* (`a2a.proto`, `SendMessageConfiguration`), so
    /// the id is filled in from the task just created rather than required
    /// from the caller. The reference implementation registers it at the same
    /// point — before the executor starts — so the very first status
    /// transition is already covered.
    ///
    /// Must run *after* the task is saved, because the config store rejects a
    /// config for a task that does not exist, and *before* the executor is
    /// spawned, so no event can be produced while the webhook is unroutable.
    ///
    /// A no-op when the request carried no config. A failure rolls back the
    /// queue and token exactly as a store failure does: a client that asked
    /// for push and did not get it must not receive a task that silently
    /// never notifies.
    ///
    /// # Errors
    ///
    /// Whatever the standalone push-config create would return.
    pub(super) async fn register_inline_push_config(
        &self,
        configuration: Option<&SendMessageConfiguration>,
        task_id: &TaskId,
    ) -> ServerResult<()> {
        let Some(inline) = configuration.and_then(|c| c.task_push_notification_config.clone())
        else {
            return Ok(());
        };
        // Shares the standalone create's validation — capability check, task
        // existence, SSRF screening, quotas — so this cannot become an
        // unguarded back door into the push config store.
        let stored = self
            .validate_and_store_push_config(TaskPushNotificationConfig {
                task_id: Some(task_id.0.clone()),
                ..inline
            })
            .await;
        if let Err(e) = stored {
            self.release_admission(task_id).await;
            // The task row is already written by this point — the push config
            // is validated against it, so it has to be. Releasing only the
            // queue and the token left a task parked in `Submitted` for ever:
            // the retention sweeps delete terminal states only, so on SQLite
            // and Postgres nothing ever collects it, and it stays visible to
            // `tasks/get` and `tasks/list`. Worse, the caller's key was
            // released too, so their retry created a *second* task and they
            // ended up with two ids for one logical send.
            //
            // Safe to delete: the executor has not been spawned and the
            // context guard is still held, so nothing has been able to
            // observe this task.
            if let Err(_delete_err) = self.task_store.delete(task_id).await {
                // Best effort. The caller is already receiving an error, and
                // an orphan row is a smaller wrong than reporting success.
                trace_warn!(
                    task_id = %task_id,
                    error = %_delete_err,
                    "push config rejected the send, and rolling the task row back failed"
                );
            }
            return Err(e);
        }
        Ok(())
    }

    /// Releases the queue and cancellation token admitted for a task whose
    /// commit failed part-way.
    async fn release_admission(&self, task_id: &TaskId) {
        self.event_queue_manager.destroy(task_id).await;
        self.cancellation_tokens.write().await.remove(task_id);
    }
}
