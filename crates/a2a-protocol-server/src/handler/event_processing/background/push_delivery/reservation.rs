// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An event's outbox rows exist before the event is stored (ADR 0037,
//! audit H14).
//!
//! Until 2026-11-11 a lane wrote an event's rows after the event was stored,
//! so a process that stopped between the two writes lost that event's
//! deliveries: the window was the time the event waited in the lane, and a
//! slow outbox database widened it. Now the background processor
//! **reserves** the rows (`PushOutbox::reserve`: unarmed, never claimed,
//! holding their webhooks' places) before the event's first durable write,
//! runs the fold, and then:
//!
//! * if the fold asked for a push (`PushLanes::push` was called inside the
//!   fold's scope, which is exactly when the event is stored and the state
//!   machine wants it announced), **arms** the reservation;
//! * otherwise (the store refused it, the write failed, the state machine
//!   announces nothing for it) **releases** it.
//!
//! A process that stops in between leaves rows with a hold that runs out.
//! Any replica's worker then settles them with [`verdict`]: from the task
//! store, was the event stored? That is a read of two things the store
//! already keeps for subscribers: the position its stored task reflects
//! (`TaskStore::get_with_position`) and the event at the reserved position
//! in the task's log (`TaskStore::read_events`).
//!
//! # What this guarantees, and what it does not
//!
//! Every event that reaches the store through the background processor and
//! that the state machine pushes has its rows in the outbox by the time it is
//! stored, or has them recorded afresh when the process that stored it comes
//! back to find its reservation settled (below). It does **not** cover:
//!
//! * a store without an event log (the position is how an event is
//!   recognised), or an outbox that keeps no reservations: both keep the
//!   lane's after-the-fact record;
//! * an outbox that is down when the event is reserved: the lane records it
//!   afterwards, as before, and the rows of two events can then reach the
//!   outbox out of order (a webhook sees them in the order the outbox got
//!   them);
//! * a verdict push (a task ended by a panic, a cancel from another
//!   replica, the claim reaper), which is not an event the processor
//!   received, and the blocking `SendMessage` path's batch.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::TaskId;

use super::lanes::{Lanes, PushLanes};
use super::outbox_worker;

tokio::task_local! {
    /// Set by `PushLanes::push` while a fold runs inside a reservation's
    /// scope: the fold has decided the event is announced.
    static WANTED: Arc<AtomicBool>;
}

/// Whether a `push` is being made inside a reservation's scope, in which case
/// it is only noted: the reservation, not the push, carries the delivery.
pub(in crate::handler::event_processing) fn requested() -> bool {
    WANTED
        .try_with(|wanted| wanted.store(true, Ordering::SeqCst))
        .is_ok()
}

/// Events the state machine can push: a status or an artifact update. A
/// snapshot or a message never is, and reserving for one would write rows to
/// remove them.
const fn may_push(event: &StreamResponse) -> bool {
    matches!(
        event,
        StreamResponse::StatusUpdate(_) | StreamResponse::ArtifactUpdate(_)
    )
}

/// Rows reserved for one event, to be settled by [`settle`](Self::settle).
pub struct Reservation {
    lanes: Arc<Lanes>,
    id: String,
    task_id: TaskId,
    event: StreamResponse,
    wanted: Arc<AtomicBool>,
}

impl PushLanes {
    /// Reserves `event`'s deliveries in the outbox, ahead of storing it at
    /// `seq` in its task's log; `None` when there is nothing to reserve or
    /// nothing to reserve with (see the module documentation), in which case
    /// the event's push is the lane's, after the event is stored.
    pub async fn reserve(
        &self,
        task_id: &TaskId,
        seq: Option<u64>,
        event: &StreamResponse,
    ) -> Option<Reservation> {
        let lanes = self.0.as_ref()?;
        let outboxed = lanes.outbox.as_ref()?;
        let (seq, tasks) = (seq?, lanes.tasks.as_ref()?);
        if !outboxed.outbox.supports_reservations()
            || !may_push(event)
            || !tasks.supports_event_log()
        {
            return None;
        }
        let configs =
            super::list_configs(task_id, &*lanes.store, &lanes.limits, &*lanes.metrics).await?;
        let rows = outbox_worker::rows(task_id, &configs, std::slice::from_ref(event));
        if rows.is_empty() {
            return None;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let reserved = outboxed
            .outbox
            .reserve(
                &id,
                seq,
                rows,
                lanes.limits.push_queue_capacity,
                outbox_worker::hold(&lanes.limits),
            )
            .await;
        match reserved {
            Ok(dropped) => {
                for _ in 0..dropped {
                    lanes
                        .metrics
                        .on_push_delivery(crate::metrics::push_outcome::DROPPED);
                }
            }
            Err(_err) => {
                trace_warn!(
                    task_id = %task_id,
                    error = %_err,
                    "the push outbox did not take a reservation; \
                     this event's deliveries are recorded after it is stored"
                );
                return None;
            }
        }
        // The worker also settles reservations a stopped replica left.
        self.start_worker();
        Some(Reservation {
            lanes: Arc::clone(lanes),
            id,
            task_id: task_id.clone(),
            event: event.clone(),
            wanted: Arc::new(AtomicBool::new(false)),
        })
    }
}

impl Reservation {
    /// Runs `fold`, the event's whole processing, noting whether it pushes.
    pub fn scope<F: Future>(&self, fold: F) -> impl Future<Output = F::Output> + use<F> {
        WANTED.scope(Arc::clone(&self.wanted), fold)
    }

    /// The fold is over: arms the rows if it pushed, releases them if not.
    ///
    /// A row that cannot be armed or released is left for the worker's
    /// settling, which reaches the same answer from the store; a reservation
    /// that settling already removed (the fold outlasted the hold, and the
    /// store did not yet show the event) is recorded afresh.
    pub async fn settle(self) {
        let Some(outboxed) = self.lanes.outbox.as_ref() else {
            return;
        };
        if !self.wanted.load(Ordering::SeqCst) {
            if let Err(_err) = outboxed.outbox.release(&self.id).await {
                trace_warn!(
                    task_id = %self.task_id,
                    error = %_err,
                    "the push outbox did not release a reservation; the worker will"
                );
            }
            return;
        }
        match outboxed.outbox.arm(&self.id).await {
            Ok(0) => {
                trace_warn!(
                    task_id = %self.task_id,
                    "a push reservation was settled before its event was stored; \
                     recording the deliveries afresh"
                );
                PushLanes(Some(Arc::clone(&self.lanes)))
                    .push_after_store(&self.task_id, &self.event);
            }
            Ok(_) => outboxed.wake.notify_one(),
            Err(_err) => {
                trace_warn!(
                    task_id = %self.task_id,
                    error = %_err,
                    "the push outbox did not arm a reservation; the worker will"
                );
            }
        }
    }
}

/// What to do with a reservation whose reserver is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The event is stored: deliver it.
    Arm,
    /// It is not: it never was, and never will be, so no webhook is told.
    Release,
}

/// Whether the event reserved at `log_seq` was stored, from what its task's
/// store holds.
///
/// `position` is the log position the stored task reflects (`None`: the task
/// is gone, or the store records none); `logged` is the event the log holds
/// at `log_seq`, if it holds one.
///
/// * The stored task has not reached `log_seq`: the fold never saved the
///   event. A non-terminal event is logged before it is folded, so the log
///   may hold it all the same; the task did not take it.
/// * It has, and the log holds a *different* event at that position: the
///   position was taken by another turn after the reserver stopped (the
///   numbering of a turn starts past the log's end and the stored position,
///   so this needs the reserver to have stopped before logging).
/// * It has, and the log holds this event or nothing: stored. (Nothing is
///   how a terminal event looks between its save and its log write, and how
///   an event whose log write failed looks.)
#[must_use]
pub fn verdict(
    reserved: &StreamResponse,
    log_seq: u64,
    position: Option<u64>,
    logged: Option<&StreamResponse>,
) -> Verdict {
    if position.is_none_or(|stored| stored < log_seq) {
        return Verdict::Release;
    }
    match logged {
        // `StreamResponse` has no `PartialEq`; both came through serde.
        Some(other) if serde_json::to_value(other).ok() != serde_json::to_value(reserved).ok() => {
            Verdict::Release
        }
        _ => Verdict::Arm,
    }
}

#[cfg(test)]
#[path = "reservation_tests.rs"]
mod tests;
