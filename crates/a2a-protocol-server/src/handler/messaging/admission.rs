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
use a2a_protocol_types::events::StreamResponse;
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
    Option<tokio::sync::mpsc::Receiver<A2aResult<StreamResponse>>>,
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
    /// # Errors
    ///
    /// [`ServerError::UnsupportedOperation`] while the task's executor runs.
    pub(super) async fn reject_in_flight_send(&self, task_id: &TaskId) -> ServerResult<()> {
        let blocked = self
            .cancellation_tokens
            .read()
            .await
            .get(task_id)
            .is_some_and(second_send_blocked);
        if blocked {
            return Err(ServerError::UnsupportedOperation(format!(
                "task {task_id} is already being processed; \
                         wait for it to reach input-required or a terminal state before sending again"
            )));
        }
        Ok(())
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
            } => Ok((writer, reader, persistence_rx)),
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
