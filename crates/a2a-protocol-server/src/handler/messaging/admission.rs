// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Admission: may this send start an executor for this task right now?
//!
//! Two refusals, both made before any side effect is committed so that a
//! refused request leaves nothing behind: a task that already has an
//! executor in flight, and a server at its concurrent-stream cap. The
//! tenant's own concurrency slot is taken earlier still, in
//! [`concurrency`](super::super::concurrency).

use std::sync::Arc;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::task::TaskId;

use super::super::RequestHandler;
use super::decisions::second_send_blocked;
use crate::error::{ServerError, ServerResult};
use crate::streaming::{InMemoryQueueReader, InMemoryQueueWriter, QueueLease};

/// What a successful lease hands the send path: the writer the executor will
/// own, the first reader, and the persistence receiver when a background
/// processor was requested.
pub(super) type LeasedQueue = (
    Arc<InMemoryQueueWriter>,
    InMemoryQueueReader,
    Option<tokio::sync::mpsc::Receiver<A2aResult<crate::streaming::StreamEvent>>>,
);

impl RequestHandler {
    /// Rejects a second send that targets a task already being processed.
    ///
    /// A live (non-cancelled) cancellation token means an executor is in
    /// flight for this `task_id`; a concurrent send would spawn a *second*
    /// executor and overwrite the first's token, leaving the original work
    /// uncancelable and racing on store writes. Only reachable when a client
    /// explicitly reuses a `task_id` (continuations); fresh sends generate a
    /// unique id. Must be called under the per-context lock so it is atomic
    /// with the token insert that follows.
    ///
    /// One in-flight executor is waited for rather than refused: one whose
    /// latest state parked the task at `input-required` or `auth-required`
    /// (N21). The client has been told to answer, and may do so before the
    /// executor's future has returned and released its token — a blocking
    /// response returns on the interrupted state, and a stream delivers it,
    /// without waiting for that. Refusing that continuation with "wait for
    /// input-required" contradicted the state the client had just been sent.
    /// The wait is bounded by
    /// [`executor_drain_timeout`](crate::handler::HandlerLimits::executor_drain_timeout);
    /// an executor still running past it is refused exactly as before, so
    /// two executors never run for one task.
    ///
    /// # Errors
    ///
    /// [`ServerError::UnsupportedOperation`] while the task's executor runs.
    pub(super) async fn reject_in_flight_send(&self, task_id: &TaskId) -> ServerResult<()> {
        let parked_turn = {
            let tokens = self.cancellation_tokens.read().await;
            match tokens.get(task_id) {
                Some(entry) if second_send_blocked(entry) => {
                    entry.turn.is_parked().then(|| Arc::clone(&entry.turn))
                }
                _ => return Ok(()),
            }
        };
        if let Some(turn) = parked_turn {
            // The executor releases its queue and token, then cancels
            // `finished`. A timeout falls through to the re-check, which
            // refuses if the executor is still there.
            let _ = tokio::time::timeout(
                self.limits.executor_drain_timeout,
                turn.finished.cancelled(),
            )
            .await;
            let still_blocked = self
                .cancellation_tokens
                .read()
                .await
                .get(task_id)
                .is_some_and(second_send_blocked);
            if !still_blocked {
                return Ok(());
            }
        }
        Err(ServerError::UnsupportedOperation(format!(
            "task {task_id} is already being processed; \
                     wait for it to reach input-required or a terminal state before sending again"
        )))
    }

    /// Leases the task's event queue, which must happen before any other
    /// side effect so hitting the concurrent-stream cap is detected first.
    ///
    /// Leasing distinguishes capacity exhaustion from an already-existing
    /// queue (see [`QueueLease`]); the old `get_or_create` collapsed both to a
    /// `None` reader, so a cap rejection was misreported as an internal error
    /// and left the task orphaned in `Submitted` with a leaked token.
    ///
    /// # Errors
    ///
    /// * [`ServerError::UnsupportedOperation`] when a queue already exists
    ///   for the task. That means either a concurrent send is racing us, or a
    ///   previous executor's queue outlived its cancelled/swept token.
    ///   Proceeding down the old `Existing` path spawned a SECOND executor
    ///   sharing the queue with NO persistence channel — silently dropping
    ///   every state transition and push notification for the resent task
    ///   (it was stuck in `Submitted`) while racing the original executor on
    ///   store writes. Rejecting is the alternative to corrupting state.
    /// * [`ServerError::Overloaded`] at the concurrent-stream cap.
    pub(super) async fn lease_event_queue(
        &self,
        task_id: &TaskId,
        use_background: bool,
    ) -> ServerResult<LeasedQueue> {
        let capacity = self
            .tenant_limits()
            .and_then(|limits| limits.event_queue_capacity);
        match self
            .event_queue_manager
            .lease(task_id, use_background, capacity)
            .await
        {
            QueueLease::Created {
                writer,
                reader,
                persistence_rx,
            } => {
                // Resume the log's numbering before the executor can write.
                // The queue is per-turn, but the log is per-task: a task
                // parked at `input-required` and then continued gets a fresh
                // writer, and since appends are idempotent *by position* a
                // counter restarting at 1 would collide with the previous
                // turn's positions and have every event of this turn silently
                // dropped. Here, and not inside `write`, because this is the
                // one moment no event can be in flight.
                if self.task_store.supports_event_log() {
                    // A failed read must not become `0`. Seeding at 0 restarts
                    // the numbering at 1, and because appends are idempotent
                    // *by position* every event of this turn then lands on a
                    // position the previous turn already holds and is swallowed
                    // as a replay — the exact silent loss the seeding exists to
                    // prevent, and invisible, because the sequence has no gap
                    // to show for it. A refused send is recoverable by retrying;
                    // a turn whose events were never recorded is not.
                    match self.task_store.last_event_seq(task_id).await {
                        Ok(seq) => writer.seed_seq(seq),
                        Err(e) => {
                            // The lease is already registered, so returning
                            // without releasing it would wedge the task as
                            // `Existing` for every later send.
                            drop((writer, reader, persistence_rx));
                            self.event_queue_manager.destroy(task_id).await;
                            return Err(ServerError::Internal(format!(
                                "task {task_id}: could not read the event log's last \
                                 position, so this turn's events could not be numbered \
                                 without colliding with the previous turn's: {e}"
                            )));
                        }
                    }
                }
                Ok((writer, reader, persistence_rx))
            }
            QueueLease::Existing => Err(ServerError::UnsupportedOperation(format!(
                "task {task_id} is already being processed; wait for it to reach \
                     input-required or a terminal state before sending again"
            ))),
            QueueLease::CapacityExhausted => {
                let cap = self
                    .event_queue_manager
                    .max_concurrent_queues()
                    .map_or_else(String::new, |n| format!(" ({n})"));
                Err(ServerError::Overloaded(format!(
                    "server at maximum concurrent stream capacity{cap}; retry later"
                )))
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use a2a_protocol_types::error::{A2aError, A2aResult};
    use a2a_protocol_types::params::ListTasksParams;
    use a2a_protocol_types::responses::TaskListResponse;
    use a2a_protocol_types::task::{Task, TaskId};

    use crate::builder::RequestHandlerBuilder;
    use crate::error::ServerError;
    use crate::executor::AgentExecutor;
    use crate::request_context::RequestContext;
    use crate::store::TaskStore;
    use crate::streaming::EventQueueWriter;

    struct NoopExecutor;

    impl AgentExecutor for NoopExecutor {
        fn execute<'a>(
            &'a self,
            _ctx: &'a RequestContext,
            _queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
    }

    /// A store that keeps a log but cannot say where it is — a pool that has
    /// gone away, or `SQLITE_BUSY` under contention.
    struct UnreadableLogStore;

    impl TaskStore for UnreadableLogStore {
        fn save<'a>(
            &'a self,
            _t: &'a Task,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
        fn get<'a>(
            &'a self,
            _id: &'a TaskId,
        ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
            Box::pin(async { Ok(None) })
        }
        fn list<'a>(
            &'a self,
            _p: &'a ListTasksParams,
        ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
            Box::pin(async { Ok(TaskListResponse::new(vec![])) })
        }
        fn insert_if_absent<'a>(
            &'a self,
            _t: &'a Task,
        ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
            Box::pin(async { Ok(true) })
        }
        fn delete<'a>(
            &'a self,
            _id: &'a TaskId,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
        fn supports_event_log(&self) -> bool {
            true
        }
        fn last_event_seq<'a>(
            &'a self,
            _id: &'a TaskId,
        ) -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>> {
            Box::pin(async { Err(A2aError::internal("the pool is gone")) })
        }
    }

    /// The defect this guards: the seed used to be `.unwrap_or(0)`. A store
    /// that could not report its last position therefore restarted the
    /// numbering at 1, and because appends are idempotent *by position* every
    /// event of the continuation landed on a position the first turn already
    /// held and was swallowed — a whole turn missing from the log, with a
    /// dense sequence and no error to show for it.
    #[tokio::test]
    async fn an_unreadable_log_position_refuses_the_lease_rather_than_renumbering() {
        let handler = RequestHandlerBuilder::new(NoopExecutor)
            .with_task_store(UnreadableLogStore)
            .build()
            .expect("handler");

        let task_id = TaskId("t-1".to_owned());
        let err = handler
            .lease_event_queue(&task_id, true)
            .await
            .expect_err("an unreadable log position must refuse the lease");

        assert!(
            matches!(err, ServerError::Internal(_)),
            "expected an internal error, got {err:?}"
        );

        // And the refusal must not wedge the task: the queue it created has to
        // be released, or every later send for this id reports "already being
        // processed" forever.
        assert_eq!(
            handler.event_queue_manager.active_count().await,
            0,
            "the failed lease must leave no queue behind"
        );
    }
}
