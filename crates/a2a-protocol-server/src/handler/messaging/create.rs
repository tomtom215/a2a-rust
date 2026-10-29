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
use super::decisions::TASK_HISTORY_LIMIT;
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
    let continuation = stored_task.filter(|_| continues(task_id, stored_task));
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

/// Whether a turn on `task_id` continues `stored_task`, the task found for
/// the message's context: only when the ids match. `resolve_task_id`
/// returns the stored id for a continuation (§3.4.3) and a fresh one
/// otherwise. One decision, read by the task built for the turn, the queue's
/// fold of its messages (`message_fold`) and the executor's span.
pub(super) fn continues(task_id: &TaskId, stored_task: Option<&Task>) -> bool {
    stored_task.is_some_and(|s| s.id == *task_id)
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

/// Most referenced tasks resolved for one message. `referenceTaskIds` is
/// client input, so without a bound one message could make the handler read
/// and hold any number of tasks.
pub(super) const MAX_RESOLVED_REFERENCES: usize = 16;

impl RequestHandler {
    /// The tasks `message.referenceTaskIds` names, read in the caller's own
    /// partition, so a task another caller or tenant holds is not found;
    /// unknown ids are skipped, duplicates read once, and at most
    /// [`MAX_RESOLVED_REFERENCES`] are read (spec §3.4.3: "Agents SHOULD use
    /// referenced tasks to understand the context and intent of follow-up
    /// requests").
    ///
    /// # Errors
    ///
    /// The store's error, converted.
    pub(super) async fn resolve_references(&self, message: &Message) -> ServerResult<Vec<Task>> {
        let mut seen: Vec<&TaskId> = Vec::new();
        let mut found = Vec::new();
        for id in message.reference_task_ids.iter().flatten() {
            if seen.len() == MAX_RESOLVED_REFERENCES {
                break;
            }
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            if let Some(task) = self.task_store.get(id).await? {
                found.push(task);
            }
        }
        Ok(found)
    }

    /// Persists the initial task. If the store refuses it, the queue and
    /// token admitted for it are released, so a store error leaks neither.
    ///
    /// `read_at` is the log position of the read a continuation's `task` was
    /// built from, which the write records (`basis`); `None` keeps the
    /// stored one.
    ///
    /// # Errors
    ///
    /// The store's error, converted.
    pub(super) async fn persist_initial_task(
        &self,
        task: &Task,
        read_at: Option<u64>,
        whole: bool,
    ) -> ServerResult<()> {
        // `whole`: `task` is the row to store (`basis::based_on`), saved as
        // it is. Otherwise `task.history` holds only this turn's message (see
        // `build_initial_task`), and that is what gets appended; the store
        // keeps the conversation.
        let appended = task.history.clone().unwrap_or_default();
        let save = if whole {
            self.task_store.save(task)
        } else {
            self.task_store
                .save_appending_history(task, &appended, TASK_HISTORY_LIMIT)
        };
        if let Err(e) = crate::store::applied::scope_if(read_at, save).await {
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
    /// Runs *before* the executor is spawned, so no event can be produced
    /// while the webhook is unroutable. For a new task (`created`) it runs
    /// *after* the task is saved, because the config store rejects a config
    /// for a task that does not exist. A continuation's task exists already,
    /// so it runs before the turn touches the row, and a refusal leaves the
    /// task as it was. Until 2026-10-13 it ran after, and the refusal's
    /// rollback deleted the task the client was continuing.
    ///
    /// A no-op when the request carried no config. A failure rolls back the
    /// queue and token exactly as a store failure does, and the row when
    /// `created`: a client that asked for push and did not get it must not
    /// receive a task that silently never notifies.
    ///
    /// Returns the config stored, for
    /// [`withdraw_inline_push_config`](Self::withdraw_inline_push_config).
    ///
    /// # Errors
    ///
    /// Whatever the standalone push-config create would return.
    pub(super) async fn register_inline_push_config(
        &self,
        configuration: Option<&SendMessageConfiguration>,
        task_id: &TaskId,
        created: bool,
    ) -> ServerResult<Option<TaskPushNotificationConfig>> {
        let Some(inline) = configuration.and_then(|c| c.task_push_notification_config.clone())
        else {
            return Ok(None);
        };
        let named = inline.id.is_some();
        // Shares the standalone create's validation — capability check, task
        // existence, SSRF screening, quotas — so this cannot become an
        // unguarded back door into the push config store.
        let stored = self
            .validate_and_store_push_config(TaskPushNotificationConfig {
                task_id: Some(task_id.0.clone()),
                ..inline
            })
            .await;
        let refused = match stored {
            // Withdrawn only if this send created it: one the request named
            // by id may have replaced a config the client already had.
            Ok(config) => return Ok((!named).then_some(config)),
            Err(e) => e,
        };
        self.release_admission(task_id).await;
        if created {
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
        }
        Err(refused)
    }

    /// Saves the turn's task. A continuation's inline push config is
    /// registered first, so that a refusal leaves the task it continues as it
    /// was, and withdrawn if the save then fails; a new task's is registered
    /// by the caller once the row exists (see
    /// [`register_inline_push_config`](Self::register_inline_push_config)).
    ///
    /// # Errors
    ///
    /// The registration's refusal, or the store's error.
    pub(super) async fn save_turn(
        &self,
        task: &Task,
        read_at: Option<u64>,
        configuration: Option<&SendMessageConfiguration>,
        continues: bool,
        whole: bool,
    ) -> ServerResult<()> {
        let registered = if continues {
            self.register_inline_push_config(configuration, &task.id, false)
                .await?
        } else {
            None
        };
        if let Err(e) = self.persist_initial_task(task, read_at, whole).await {
            self.withdraw_inline_push_config(registered).await;
            return Err(e);
        }
        Ok(())
    }

    /// Records the approval the gate admitted for this send, if any, in the
    /// audit log, if there is one.
    #[cfg(feature = "audit")]
    pub(super) async fn audit_approval(
        &self,
        call_ctx: &crate::call_context::CallContext,
        task_id: &TaskId,
        approval: Option<&crate::approval::VerifiedApproval>,
    ) {
        if let (Some(log), Some(approval)) = (&self.audit, approval) {
            crate::audit::record_approval(log, &*self.metrics, call_ctx, task_id, approval).await;
        }
    }

    /// Withdraws the config a continuation registered when its turn then
    /// could not be saved. The client is told the send failed, and a retry
    /// registers the config again; left in place, the retry's would be a
    /// second one delivering every notification twice.
    pub(super) async fn withdraw_inline_push_config(
        &self,
        config: Option<TaskPushNotificationConfig>,
    ) {
        let Some(TaskPushNotificationConfig {
            task_id: Some(task_id),
            id: Some(id),
            ..
        }) = config
        else {
            return;
        };
        if let Err(_e) = self.push_config_store.delete(&task_id, &id).await {
            trace_warn!(
                task_id = %task_id,
                error = %_e,
                "the turn was not saved, and withdrawing its inline push config failed"
            );
        }
    }

    /// Releases the queue and cancellation token admitted for a task whose
    /// commit failed part-way.
    async fn release_admission(&self, task_id: &TaskId) {
        self.event_queue_manager.destroy(task_id).await;
        self.cancellation_tokens.write().await.remove(task_id);
    }
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

    use super::*;

    fn task(id: &str) -> Task {
        Task {
            id: TaskId::new(id),
            context_id: ContextId::new("c"),
            status: TaskStatus::new(TaskState::InputRequired),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    #[test]
    fn a_turn_continues_only_the_task_it_names() {
        let stored = task("t-1");
        assert!(continues(&TaskId::new("t-1"), Some(&stored)));
        assert!(
            !continues(&TaskId::new("t-2"), Some(&stored)),
            "another task in the context"
        );
        assert!(!continues(&TaskId::new("t-1"), None));
    }
}
