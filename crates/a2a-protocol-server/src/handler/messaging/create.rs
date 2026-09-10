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
/// A continuation carries the stored task's accumulated history, artifacts,
/// and metadata forward — only the status returns to Submitted for the new
/// turn. The incoming message is appended to `history` in both cases:
/// `Task.history` is the conversation record that `GetTask`'s
/// `historyLength` truncates, and multi-turn executors read prior turns from
/// it via `RequestContext::stored_task`.
pub(super) fn build_initial_task(
    task_id: &TaskId,
    context_id: &str,
    stored_task: Option<&Task>,
    message: &Message,
) -> Task {
    let mut history = stored_task
        .and_then(|s| s.history.clone())
        .unwrap_or_default();
    history.push(message.clone());
    // Unguarded: at or under the cap `excess` is 0 and `drain(..0)` costs
    // nothing — `Drain::drop` skips its memmove when the tail does not
    // move, so this is O(1), not an O(n) shift of the whole history. The
    // `if` it replaces guarded only that no-op, which is precisely what
    // made weakening it to `>=` an equivalent mutant: both arms did
    // nothing at `len == MAX`.
    let excess = history.len().saturating_sub(MAX_TASK_HISTORY_MESSAGES);
    history.drain(..excess);
    Task {
        id: task_id.clone(),
        context_id: ContextId::new(context_id),
        status: TaskStatus::with_timestamp(TaskState::Submitted),
        history: Some(history),
        artifacts: stored_task.and_then(|s| s.artifacts.clone()),
        metadata: stored_task.and_then(|s| s.metadata.clone()),
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
) -> RequestContext {
    let mut ctx = RequestContext::new(message, task_id, context_id);
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
        if let Err(e) = self.task_store.save(task).await {
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
