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

use a2a_protocol_types::error::A2aError;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::failure::{FailureClass, error_class, set_class};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
use tokio::sync::OwnedSemaphorePermit;
use tokio::task::JoinHandle;

use crate::executor::AgentExecutor;
use crate::request_context::RequestContext;
use crate::streaming::{EventQueueManager, EventQueueWriter, InMemoryQueueWriter};

use super::terminal::TerminalTracking;

use super::super::{CancellationEntry, ExecutorTurn, RequestHandler};

/// The handler's cancellation-token map, as the spawned task holds it.
pub(super) type CancellationTokens = Arc<tokio::sync::RwLock<HashMap<TaskId, CancellationEntry>>>;

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
    turn: Arc<ExecutorTurn>,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let Some(tid) = self.task_id.take() {
            let qmgr = self.queue_mgr.clone();
            let tokens = Arc::clone(&self.tokens);
            let turn = Arc::clone(&self.turn);
            tokio::task::spawn(async move {
                qmgr.destroy(&tid).await;
                tokens.write().await.remove(&tid);
                turn.finished.cancel();
            });
        }
    }
}

/// A blocking send's executor, built at commit and run by the task that
/// collects its events — one task per blocking send instead of two.
///
/// Nothing in the executor's future runs, and so nothing in it is armed,
/// until it is first polled: dropped unrun, the task's queue, token and turn
/// would never be released and the task would stay `Submitted`. So `Drop`
/// spawns it exactly as [`RequestHandler::spawn_executor`] would have, and
/// the only way to skip that is to take it with [`Self::watch`].
pub struct DeferredExecutor {
    run: Option<ExecutorRun>,
    tracker: tokio_util::task::TaskTracker,
}

/// The executor's whole run, as [`RequestHandler::executor_future`] builds it.
type ExecutorRun = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

impl DeferredExecutor {
    /// Spawns the executor on the handler's tracker, as
    /// [`RequestHandler::spawn_executor`] does.
    pub fn spawn(mut self) -> JoinHandle<()> {
        self.run.take().map_or_else(
            || self.tracker.spawn(async {}),
            |run| self.tracker.spawn(run),
        )
    }

    /// Takes the executor, to be run in the caller's own task.
    pub fn watch(mut self) -> WatchedExecutor {
        WatchedExecutor::Inline(self.run.take())
    }
}

impl Drop for DeferredExecutor {
    fn drop(&mut self) {
        if let Some(run) = self.run.take() {
            self.tracker.spawn(run);
        }
    }
}

/// An executor as the sync collector watches it: resolves once the executor
/// has ended, to `true` when it returned and `false` when it panicked or was
/// aborted.
pub enum WatchedExecutor {
    /// Polled from the watcher's own task; `None` once it has ended.
    Inline(Option<ExecutorRun>),
    /// Running on a task of its own; `None` once it has ended.
    Spawned(Option<JoinHandle<()>>),
}

impl std::future::Future for WatchedExecutor {
    type Output = bool;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<bool> {
        use std::task::Poll;
        match &mut *self {
            Self::Inline(slot) => {
                let Some(run) = slot.as_mut() else {
                    return Poll::Ready(true);
                };
                // `AssertUnwindSafe`: after a panic the future is never polled
                // again — it is dropped right here, which is what tokio does
                // with a panicked task, and what releases the executor's
                // `CleanupGuard`. Nothing observes its state in between.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run.as_mut().poll(cx)
                })) {
                    Ok(Poll::Pending) => Poll::Pending,
                    Ok(Poll::Ready(())) => {
                        *slot = None;
                        Poll::Ready(true)
                    }
                    Err(_panic) => {
                        *slot = None;
                        Poll::Ready(false)
                    }
                }
            }
            Self::Spawned(slot) => {
                let Some(handle) = slot.as_mut() else {
                    return Poll::Ready(true);
                };
                match std::pin::Pin::new(handle).poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(joined) => {
                        *slot = None;
                        Poll::Ready(joined.is_ok())
                    }
                }
            }
        }
    }
}

impl WatchedExecutor {
    /// Runs an inline executor to its end. A blocking send answers as soon
    /// as the task is terminal or interrupted, which can be before the
    /// executor returns; its cleanup still has to run. A spawned executor
    /// ends on its own task.
    pub async fn finish(self) {
        if let Self::Inline(Some(run)) = self {
            run.await;
        }
    }
}

impl RequestHandler {
    /// Spawns the executor for a task whose queue, token and row all exist.
    pub(super) fn spawn_executor(
        &self,
        ctx: RequestContext,
        writer: Arc<InMemoryQueueWriter>,
        tenant_slot: Option<OwnedSemaphorePermit>,
        turn: Arc<ExecutorTurn>,
    ) -> JoinHandle<()> {
        self.in_flight
            .executors()
            .spawn(self.executor_future(ctx, writer, tenant_slot, turn))
    }

    /// Builds the executor for a task whose queue, token and row all exist,
    /// to be run by the task that collects its events.
    pub(super) fn defer_executor(
        &self,
        ctx: RequestContext,
        writer: Arc<InMemoryQueueWriter>,
        tenant_slot: Option<OwnedSemaphorePermit>,
        turn: Arc<ExecutorTurn>,
    ) -> DeferredExecutor {
        DeferredExecutor {
            run: Some(Box::pin(self.executor_future(
                ctx,
                writer,
                tenant_slot,
                turn,
            ))),
            tracker: self.in_flight.executors().clone(),
        }
    }

    /// The executor's whole run, from its first poll to its cleanup.
    ///
    /// The future owns the only writer clone needed, so the channel closes —
    /// and readers see EOF — when the executor finishes. It also owns the
    /// tenant's concurrency permit, which is returned when the future ends,
    /// however it ends: dropping the future drops the permit.
    fn executor_future(
        &self,
        ctx: RequestContext,
        writer: Arc<InMemoryQueueWriter>,
        tenant_slot: Option<OwnedSemaphorePermit>,
        turn: Arc<ExecutorTurn>,
    ) -> impl std::future::Future<Output = ()> + Send + 'static {
        let executor = Arc::clone(&self.executor);
        let task_id = ctx.task_id.clone();
        let event_queue_mgr = self.event_queue_manager.clone();
        let cancel_tokens = Arc::clone(&self.cancellation_tokens);
        // Resolved here, not inside the future: `TenantContext` is a task-local
        // and `tokio::spawn` does not inherit it. A per-tenant override wins
        // over the handler-wide default; `None` on the tenant means "use the
        // handler's", which is what the field has always documented.
        let executor_timeout = self
            .tenant_limits()
            .and_then(|limits| limits.executor_timeout)
            .or(self.executor_timeout);

        // Captured for the same reason and re-entered below. Without this the
        // executor — and every store call it makes — ran under the empty
        // tenant, so a tenant-aware store partitioned the executor's writes
        // away from the request that caused them. The background event
        // processor and the sync collector already do exactly this; this
        // spawn was the one that did not.
        let tenant = crate::store::tenant::TenantContext::current();
        let shutdown = self.in_flight.shutdown_token();

        // Read before `ctx` moves into the executor's future; the span
        // records them when it is created.
        let span_ids = (ctx.task_id.to_string(), ctx.context_id.clone());
        crate::rpc_span::in_executor_span(
            &span_ids.0,
            &span_ids.1,
            crate::store::tenant::TenantContext::scope(tenant, async move {
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
                    turn: Arc::clone(&turn),
                };

                let writer = TerminalTracking::new(writer, Arc::clone(&turn));
                let result = run_executor(executor.as_ref(), &ctx, &writer, executor_timeout).await;
                if let Err((ref e, class)) = result {
                    write_failure_event(&writer, &ctx, e, class).await;
                } else if shutdown.is_cancelled() && !writer.terminal_written() {
                    // Shut down, not cancelled by a caller: `CancelTask` runs
                    // this hook itself, and cancels only the task's own child
                    // token. The executor saw its token and returned without a
                    // terminal state, so end the task the way `CancelTask`
                    // would — the default hook writes `Canceled` — and every
                    // stream still open on it gets a terminal event instead of
                    // simply stopping. After `execute` returned, never beside
                    // it: an executor that wrote its own terminal state while
                    // the hook wrote another would have the second rejected as
                    // an invalid transition and the task marked `Failed`.
                    if let Err(_e) = executor.cancel(&ctx, &writer).await {
                        trace_warn!(
                            task_id = %ctx.task_id,
                            error = %_e,
                            "cancel hook failed during shutdown"
                        );
                    }
                }
                // Drop the writer so the channel closes and readers see EOF.
                drop(writer);
                // Explicit cleanup, then disarm the guard so it does not release
                // a second time on normal exit.
                event_queue_mgr.destroy(&task_id).await;
                cancel_tokens.write().await.remove(&task_id);
                cleanup_guard.task_id = None;
                // Last: a continuation waiting in admission may now lease
                // a queue and register a token under the same id.
                turn.finished.cancel();
            }),
        )
    }
}

/// Runs the executor under the resolved timeout, if there is one.
///
/// A timeout is reported as an internal error, so it takes the same failure
/// path as an executor that returned `Err`. An executor's own error is
/// classified by [`error_class`]: a class the executor recorded on it with
/// `set_error_class` (as the client's `From<ClientError>` does for a
/// delegated call's timeout) wins over the one its code implies.
async fn run_executor(
    executor: &dyn AgentExecutor,
    ctx: &RequestContext,
    writer: &dyn EventQueueWriter,
    timeout: Option<Duration>,
) -> Result<(), (A2aError, FailureClass)> {
    let Some(timeout) = timeout else {
        return executor
            .execute(ctx, writer)
            .await
            .map_err(|e| (error_class(&e), e))
            .map_err(|(class, e)| (e, class));
    };
    match tokio::time::timeout(timeout, executor.execute(ctx, writer)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            let class = error_class(&e);
            Err((e, class))
        }
        // A deadline is a bound that was hit, not an agent that broke, and
        // the classes exist so a caller can tell those apart: an identical
        // retry hits the identical deadline, a retry with more budget need
        // not.
        Err(_) => Err((
            A2aError::internal(format!("executor timed out after {}s", timeout.as_secs())),
            FailureClass::BudgetExhausted,
        )),
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
async fn write_failure_event(
    writer: &dyn EventQueueWriter,
    ctx: &RequestContext,
    error: &A2aError,
    class: FailureClass,
) {
    trace_error!(task_id = %ctx.task_id, error = %error, "executor failed");
    let fail_event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: ctx.task_id.clone(),
        context_id: ContextId::new(ctx.context_id.clone()),
        status: failure_status(ctx, error, class),
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
fn failure_status(ctx: &RequestContext, error: &A2aError, class: FailureClass) -> TaskStatus {
    let mut status = TaskStatus::with_timestamp(TaskState::Failed);
    let mut note = Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::Agent,
        parts: vec![Part::text(error.to_string())],
        task_id: Some(ctx.task_id.clone()),
        context_id: Some(ContextId::new(ctx.context_id.clone())),
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    };
    // The class rides the status message beside the prose, so a caller can
    // branch on a value instead of matching English. An executor that
    // classified for itself emits its own terminal status and never reaches
    // here.
    set_class(&mut note, class);
    status.message = Some(note);
    status
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{DeferredExecutor, WatchedExecutor};

    /// Dropped unrun, a deferred executor is spawned rather than lost: its
    /// cleanup guard is armed only once it runs, so losing it would leak
    /// the task's queue and token and leave the task `Submitted`.
    #[tokio::test]
    async fn a_deferred_executor_dropped_unrun_still_runs() {
        let ran = Arc::new(AtomicBool::new(false));
        let tracker = tokio_util::task::TaskTracker::new();
        let deferred = DeferredExecutor {
            run: Some(Box::pin({
                let ran = Arc::clone(&ran);
                async move { ran.store(true, Ordering::SeqCst) }
            })),
            tracker: tracker.clone(),
        };
        drop(deferred);
        tracker.close();
        tracker.wait().await;
        assert!(ran.load(Ordering::SeqCst));
    }

    /// Taken with `watch`, it is not spawned a second time on drop.
    #[tokio::test]
    async fn a_watched_executor_runs_once_inline() {
        let tracker = tokio_util::task::TaskTracker::new();
        let deferred = DeferredExecutor {
            run: Some(Box::pin(async {})),
            tracker: tracker.clone(),
        };
        let mut watched = deferred.watch();
        assert!(tracker.is_empty(), "watching does not spawn");
        assert!(
            (&mut watched).await,
            "a returning executor resolves to true"
        );
        watched.finish().await;
        assert!(tracker.is_empty());
    }

    #[tokio::test]
    async fn an_inline_panic_resolves_to_false() {
        let mut watched = WatchedExecutor::Inline(Some(Box::pin(async { panic!("inline panic") })));
        assert!(!(&mut watched).await);
    }

    #[tokio::test]
    async fn a_spawned_executor_reports_its_join_result() {
        let mut ok = WatchedExecutor::Spawned(Some(tokio::spawn(async {})));
        assert!((&mut ok).await);
        let mut panicked =
            WatchedExecutor::Spawned(Some(tokio::spawn(async { panic!("spawned panic") })));
        assert!(!(&mut panicked).await);
    }
}
