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
use crate::store::tenant::TenantContext;

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
    /// `None` when the outbox did not take the reservation: the event is then
    /// recorded in order, awaited, once the fold announces it.
    id: Option<String>,
    /// The config and delivery id of each reserved row, so that rows recorded
    /// again (the reservation was settled before the event was stored) keep the
    /// ids a receiver may already have seen.
    ids: Vec<(String, String)>,
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
        let (seq, tasks) = (seq?, lanes.tasks.as_ref()?);
        if !may_push(event) || !tasks.supports_event_log() {
            return None;
        }
        self.reserve_at(task_id, seq, event).await
    }

    /// Reserves the deliveries of a task's **ending**, ahead of saving it: a
    /// terminal status update no queued event carries (an executor's panic).
    /// It has no log position yet, so it is settled by the state the task is
    /// stored in (`ending_verdict`), which a terminal state never leaves.
    pub async fn reserve_ending(
        &self,
        task_id: &TaskId,
        event: &StreamResponse,
    ) -> Option<Reservation> {
        let StreamResponse::StatusUpdate(update) = event else {
            return None;
        };
        if !update.status.state.is_terminal() || self.0.as_ref()?.tasks.is_none() {
            return None;
        }
        self.reserve_at(task_id, ENDING, event).await
    }

    /// Reserves at log position `seq` (`ENDING` for an ending); see
    /// [`reserve`](Self::reserve).
    async fn reserve_at(
        &self,
        task_id: &TaskId,
        seq: u64,
        event: &StreamResponse,
    ) -> Option<Reservation> {
        let lanes = self.0.as_ref()?;
        let outboxed = lanes.outbox.as_ref()?;
        if !outboxed.outbox.supports_reservations() {
            return None;
        }
        let configs =
            super::list_configs(task_id, &*lanes.store, &lanes.limits, &*lanes.metrics).await?;
        let rows = outbox_worker::rows(task_id, &configs, std::slice::from_ref(event));
        if rows.is_empty() {
            return None;
        }
        let ids: Vec<(String, String)> = rows
            .iter()
            .map(|row| (row.config_id.clone(), row.delivery_id.clone()))
            .collect();
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
        let (id, ids) = match reserved {
            Ok(dropped) => {
                for _ in 0..dropped {
                    lanes
                        .metrics
                        .on_push_delivery(crate::metrics::push_outcome::DROPPED);
                }
                // The worker also settles reservations a stopped replica left.
                self.start_worker();
                (Some(id), ids)
            }
            Err(_err) => {
                // The outbox is down: nothing is reserved, but the event is
                // still recorded in its place (awaited, before the next
                // event's reservation), not queued behind it in the lane.
                trace_warn!(
                    task_id = %task_id,
                    error = %_err,
                    "the push outbox did not take a reservation; \
                     this event's deliveries are recorded when it is stored"
                );
                (None, Vec::new())
            }
        };
        Some(Reservation {
            lanes: Arc::clone(lanes),
            id,
            ids,
            task_id: task_id.clone(),
            event: event.clone(),
            wanted: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Settles every reservation of `task_id` that is not armed, whether or not
    /// its hold has run out, from what the task store holds; how many.
    ///
    /// For a caller that holds the task's turn claim after taking it over (the
    /// reaper): a store refuses a writer whose claim was taken over, so the
    /// dead replica can write no more and what the store holds is final.
    pub async fn settle_task(&self, task_id: &TaskId) -> usize {
        let Some(lanes) = self.0.as_ref() else {
            return 0;
        };
        let (Some(outboxed), Some(tasks)) = (lanes.outbox.as_ref(), lanes.tasks.as_ref()) else {
            return 0;
        };
        if !outboxed.outbox.supports_reservations() {
            return 0;
        }
        let (tenant, caller) = (TenantContext::current(), TenantContext::caller());
        let held = match outboxed
            .outbox
            .held_for_task(&tenant, caller.as_deref(), task_id.as_ref())
            .await
        {
            Ok(held) => held,
            Err(_err) => {
                trace_warn!(task_id = %task_id, error = %_err, "a task's push reservations could not be read");
                return 0;
            }
        };
        let mut settled = 0;
        for held in held {
            if super::settling::settle_one(lanes, outboxed, tasks.as_ref(), held).await {
                settled += 1;
            }
        }
        settled
    }
}

impl Reservation {
    /// Runs `fold`, the event's whole processing, noting whether it pushes.
    pub(in crate::handler::event_processing) fn scope<F: Future>(
        &self,
        fold: F,
    ) -> impl Future<Output = F::Output> + use<F> {
        WANTED.scope(Arc::clone(&self.wanted), fold)
    }

    /// The fold is over: arms the rows if it pushed, releases them if not.
    pub(in crate::handler::event_processing) async fn settle(self) {
        if self.wanted.load(Ordering::SeqCst) {
            self.arm_now().await;
        } else {
            self.release_now().await;
        }
    }

    /// The event is stored: its rows become claimable.
    ///
    /// A row that cannot be armed is left for the worker's settling, which
    /// reaches the same answer from the store. A reservation the worker already
    /// removed (the fold outlasted the hold, and the store did not yet show the
    /// event) is recorded again **under the same delivery ids**, so if the first
    /// settlement had armed it and it was delivered, the receiver sees the same
    /// id twice, which it knows for a retry. With no reservation (the outbox was
    /// down) the event is recorded now, in order.
    pub(in crate::handler::event_processing) async fn arm_now(self) {
        let Some(outboxed) = self.lanes.outbox.as_ref() else {
            return;
        };
        let lanes = PushLanes(Some(Arc::clone(&self.lanes)));
        let Some(id) = &self.id else {
            lanes.record_now(&self.task_id, &self.event).await;
            return;
        };
        match outboxed.outbox.arm(id).await {
            Ok(0) => {
                trace_warn!(
                    task_id = %self.task_id,
                    "a push reservation was settled before its event was stored; \
                     recording the deliveries again under the same ids"
                );
                outbox_worker::record_again(
                    &self.lanes,
                    outboxed,
                    &self.task_id,
                    &self.event,
                    &self.ids,
                )
                .await;
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

    /// The event was not stored: its rows are removed. A row that cannot be
    /// removed is removed by the worker's settling.
    pub(in crate::handler::event_processing) async fn release_now(self) {
        let (Some(outboxed), Some(id)) = (self.lanes.outbox.as_ref(), &self.id) else {
            return;
        };
        if let Err(_err) = outboxed.outbox.release(id).await {
            trace_warn!(
                task_id = %self.task_id,
                error = %_err,
                "the push outbox did not release a reservation; the worker will"
            );
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

/// What a reservation's `log_seq` is when it reserves a task's ending, which has
/// no position in the log yet. Real positions start at 1.
pub(super) const ENDING: u64 = 0;

/// Whether the ending reserved as `reserved` was stored: the task is stored in
/// the terminal state it names. A terminal state is never left, so this is
/// exact: another terminal state means another writer ended the task, and that
/// writer's ending is not this one.
#[must_use]
pub fn ending_verdict(
    reserved: &StreamResponse,
    stored: Option<a2a_protocol_types::task::TaskState>,
) -> Verdict {
    match (reserved, stored) {
        (StreamResponse::StatusUpdate(update), Some(state))
            if state.is_terminal() && update.status.state == state =>
        {
            Verdict::Arm
        }
        _ => Verdict::Release,
    }
}

#[cfg(test)]
#[path = "reservation_tests.rs"]
mod tests;
