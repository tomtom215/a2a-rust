// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a processor does when a write failed: read the store before saying what
//! became of the event, and keep a reservation it could not settle (adversarial
//! review R1 and R2, 2026-11-12). Part of [`Processor`](super::Processor).

use a2a_protocol_types::events::StreamResponse;

use super::super::state_machine::Outcome;
use super::super::{Fate, Reservation};
use super::Processor;
use crate::store::TerminalStateConflict;

/// The state a status update or snapshot puts the task in.
const fn state_of(event: &StreamResponse) -> Option<a2a_protocol_types::task::TaskState> {
    match event {
        StreamResponse::StatusUpdate(update) => Some(update.status.state),
        StreamResponse::Task(task) => Some(task.status.state),
        _ => None,
    }
}

impl Processor<'_> {
    /// The fold is over: what became of the event is established (reading the
    /// store if its write failed), its reservation settled, and the run told.
    pub(super) async fn finish(
        &mut self,
        outcome: Outcome,
        seq: Option<u64>,
        terminal: Option<StreamResponse>,
        reservation: Option<Reservation>,
        pushes_terminal: bool,
    ) {
        let (outcome, fate) = self
            .reconciled(
                outcome,
                terminal.as_ref(),
                reservation.as_ref(),
                pushes_terminal,
            )
            .await;
        if let Some(reservation) = reservation
            && let Some(left) = reservation.settle(fate).await
        {
            self.unsettled.push(left);
        }
        self.conclude(outcome, seq, terminal, pushes_terminal).await;
    }

    /// Settles the reservations an earlier event left, from what the store
    /// holds; those that still cannot be settled stay.
    pub(super) async fn settle_unsettled(&mut self) {
        if self.unsettled.is_empty() {
            return;
        }
        let mut left = Vec::new();
        for reservation in std::mem::take(&mut self.unsettled) {
            if !reservation.resolve().await {
                left.push(reservation);
            }
        }
        self.unsettled = left;
    }

    /// What became of an event whose write failed: **the failure does not say**.
    /// A write is applied and its acknowledgement lost as easily as it is
    /// refused, so the store is read before anything is announced
    /// (adversarial review R1, R2, 2026-11-12).
    ///
    /// * A terminal event: the task is stored in the state it names (the write
    ///   landed after all, or another writer made the same ending), in
    ///   another terminal state (another writer finished it), or not terminal
    ///   (not stored), or the store cannot be read (unknown).
    /// * A reserved event: the stored log position reaches its position, which
    ///   — with no position stored past an unsettled reservation — says the
    ///   write landed.
    ///
    /// Anything else is as the fold said.
    pub(super) async fn reconciled(
        &mut self,
        outcome: Outcome,
        terminal: Option<&StreamResponse>,
        reservation: Option<&Reservation>,
        pushes_terminal: bool,
    ) -> (Outcome, Fate) {
        if outcome != Outcome::StoreFailed {
            return (outcome, Fate::ByFold);
        }
        if let Some(event) = terminal {
            return self
                .reconciled_ending(event, reservation, pushes_terminal)
                .await;
        }
        let Some(reservation) = reservation else {
            return (outcome, Fate::ByFold);
        };
        match reservation.stored().await {
            Some(true) => match self.deps.task_store.get(self.task_id).await {
                Ok(Some(stored)) => {
                    // Applied after all: the in-memory task is the stored one,
                    // or a later whole-task write would undo the event.
                    self.last_task = stored;
                    (Outcome::Persisted, Fate::Stored)
                }
                _ => (outcome, Fate::Unknown),
            },
            Some(false) => (outcome, Fate::NotStored),
            None => (outcome, Fate::Unknown),
        }
    }

    /// [`reconciled`](Self::reconciled) for a terminal event.
    async fn reconciled_ending(
        &mut self,
        event: &StreamResponse,
        reservation: Option<&Reservation>,
        pushes_terminal: bool,
    ) -> (Outcome, Fate) {
        let attempted = state_of(event);
        let stored = match self.deps.task_store.get(self.task_id).await {
            Ok(stored) => stored,
            Err(_e) => {
                trace_warn!(
                    task_id = %self.task_id,
                    error = %_e,
                    "background processor: a terminal write failed and the task cannot be read back; \
                     the ending is not confirmed"
                );
                return (Outcome::StoreFailed, Fate::Unknown);
            }
        };
        let Some(stored) = stored.filter(|stored| stored.status.state.is_terminal()) else {
            return (Outcome::StoreFailed, Fate::NotStored);
        };
        let held = stored.status.state;
        if Some(held) != attempted {
            // Another writer finished the task in another state.
            let conflict =
                TerminalStateConflict::new(self.task_id.clone(), held, attempted.unwrap_or(held));
            return (Outcome::Refused(conflict), Fate::NotStored);
        }
        // The task is stored in the state the event names, though the write
        // reported an error: it landed, or another writer made the same ending.
        trace_warn!(
            task_id = %self.task_id,
            state = %held,
            "background processor: a terminal write reported a failure but the task is stored in \
             that state; confirming it"
        );
        self.last_task = stored;
        if reservation.is_none() && pushes_terminal {
            // The lane's push, which the fold made when its write succeeded.
            self.deps.push.push(self.task_id, event);
        }
        (Outcome::Persisted, Fate::Stored)
    }
}
