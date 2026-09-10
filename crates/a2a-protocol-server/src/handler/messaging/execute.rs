// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The executor run: spawn, timeout, failure reporting, and release.
//!
//! Everything here happens on the spawned task, after the send path has
//! committed the task row, the event queue and the cancellation token. The
//! one invariant is that those two resources are released exactly once,
//! however the executor ends — which is what [`CleanupGuard`] is for, and why
//! its lifetime is spelled out in [`RequestHandler::spawn_executor`] rather
//! than left to scope.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
use tokio::sync::OwnedSemaphorePermit;
use tokio::task::JoinHandle;

use crate::executor::AgentExecutor;
use crate::request_context::RequestContext;
use crate::streaming::{EventQueueManager, EventQueueWriter, InMemoryQueueWriter};

use super::super::{CancellationEntry, RequestHandler};

/// The handler's cancellation-token map, as the spawned task holds it.
type CancellationTokens = Arc<tokio::sync::RwLock<HashMap<TaskId, CancellationEntry>>>;

/// Releases a task's event queue and cancellation token when the executor's
/// future is dropped before reaching its explicit cleanup — a panic, or an
/// abort of the `JoinHandle` (FIX(L5)).
///
/// # Lifetime
///
/// Armed at the top of the spawned future, before the executor runs, and
/// **disarmed** (`task_id` taken) only after the explicit cleanup at the end
/// of that future has run. So exactly one of the two releases the resources:
///
/// * normal exit, success or handled error — the explicit cleanup runs, then
///   the guard is disarmed, and its `drop` is a no-op;
/// * unwind or abort — the explicit cleanup never runs, the guard drops still
///   armed, and `drop` spawns the release.
///
/// The release is spawned rather than awaited because `Drop` cannot await,
/// and it must not run twice: a resent task (an input-required continuation
/// reuses its id) may by then own a *new* queue and token under the same id,
/// which a stray second release would destroy.
struct CleanupGuard {
    task_id: Option<TaskId>,
    queue_mgr: EventQueueManager,
    tokens: CancellationTokens,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let Some(tid) = self.task_id.take() {
            let qmgr = self.queue_mgr.clone();
            let tokens = Arc::clone(&self.tokens);
            tokio::task::spawn(async move {
                qmgr.destroy(&tid).await;
                tokens.write().await.remove(&tid);
            });
        }
    }
}

impl RequestHandler {
    /// Spawns the executor for a task whose queue, token and row all exist.
    ///
    /// The spawned task owns the only writer clone needed, so the channel
    /// closes — and readers see EOF — when the executor finishes. It also
    /// owns the tenant's concurrency permit, which is returned when the
    /// future ends, however it ends: dropping the future drops the permit.
    pub(super) fn spawn_executor(
        &self,
        ctx: RequestContext,
        writer: Arc<InMemoryQueueWriter>,
        tenant_slot: Option<OwnedSemaphorePermit>,
    ) -> JoinHandle<()> {
        let executor = Arc::clone(&self.executor);
        let task_id = ctx.task_id.clone();
        let event_queue_mgr = self.event_queue_manager.clone();
        let cancel_tokens = Arc::clone(&self.cancellation_tokens);
        // Resolved here, not inside the spawn: `TenantContext` is a task-local
        // and `tokio::spawn` does not inherit it. A per-tenant override wins
        // over the handler-wide default; `None` on the tenant means "use the
        // handler's", which is what the field has always documented.
        let executor_timeout = self
            .tenant_limits()
            .and_then(|limits| limits.executor_timeout)
            .or(self.executor_timeout);

        tokio::spawn(async move {
            // Owned by this future, so the slot is returned when the executor
            // finishes, fails, panics, or is aborted.
            let _tenant_slot = tenant_slot;
            trace_debug!(task_id = %ctx.task_id, "executor started");

            // Armed before the executor runs; see the type's docs for when it
            // fires. There is no `catch_unwind` here — the guard *is* the
            // panic handling.
            let mut cleanup_guard = CleanupGuard {
                task_id: Some(task_id.clone()),
                queue_mgr: event_queue_mgr.clone(),
                tokens: Arc::clone(&cancel_tokens),
            };

            let result =
                run_executor(executor.as_ref(), &ctx, writer.as_ref(), executor_timeout).await;
            if let Err(ref e) = result {
                write_failure_event(writer.as_ref(), &ctx, e).await;
            }
            // Drop the writer so the channel closes and readers see EOF.
            drop(writer);
            // Explicit cleanup, then disarm the guard so it does not release
            // a second time on normal exit.
            event_queue_mgr.destroy(&task_id).await;
            cancel_tokens.write().await.remove(&task_id);
            cleanup_guard.task_id = None;
        })
    }
}

/// Runs the executor under the resolved timeout, if there is one.
///
/// A timeout is reported as an internal error, so it takes the same failure
/// path as an executor that returned `Err`.
async fn run_executor(
    executor: &dyn AgentExecutor,
    ctx: &RequestContext,
    writer: &dyn EventQueueWriter,
    timeout: Option<Duration>,
) -> A2aResult<()> {
    if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, executor.execute(ctx, writer))
            .await
            .unwrap_or_else(|_| {
                Err(A2aError::internal(format!(
                    "executor timed out after {}s",
                    timeout.as_secs()
                )))
            })
    } else {
        executor.execute(ctx, writer).await
    }
}

/// Writes the `Failed` status update for an executor that returned `Err`.
///
/// The error text travels twice, for two audiences. `status.message` is the
/// spec's field for "additional status updates for the client", and it is
/// what the sync collector copies onto the task, so a blocking `SendMessage`
/// — and every `GetTask` after it — returns a `Failed` task that says why.
/// `metadata.error` is where streaming callers have always read it, and it
/// stays for them.
async fn write_failure_event(writer: &InMemoryQueueWriter, ctx: &RequestContext, error: &A2aError) {
    trace_error!(task_id = %ctx.task_id, error = %error, "executor failed");
    let fail_event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: ctx.task_id.clone(),
        context_id: ContextId::new(ctx.context_id.clone()),
        status: failure_status(ctx, error),
        metadata: Some(serde_json::json!({ "error": error.to_string() })),
    });
    if let Err(_write_err) = writer.write(fail_event).await {
        trace_error!(
            task_id = %ctx.task_id,
            error = %_write_err,
            "failed to write failure event to queue"
        );
    }
}

/// The `Failed` status, carrying the error as an agent-role message with one
/// text part.
fn failure_status(ctx: &RequestContext, error: &A2aError) -> TaskStatus {
    let mut status = TaskStatus::with_timestamp(TaskState::Failed);
    status.message = Some(Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::Agent,
        parts: vec![Part::text(error.to_string())],
        task_id: Some(ctx.task_id.clone()),
        context_id: Some(ContextId::new(ctx.context_id.clone())),
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    });
    status
}
