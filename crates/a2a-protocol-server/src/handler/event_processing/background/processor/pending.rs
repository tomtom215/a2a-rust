// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A task's ending the store did not take, written again until it does
//! (COR-01, register X-10), and the breaker that keeps an outage from
//! costing every queued event the whole retry budget.
//!
//! Before: a terminal status whose write outlasted
//! [`store_write_retry`](crate::handler::HandlerLimits::store_write_retry)
//! was reverted in memory and forgotten, and the task stayed `working` in the
//! store with nothing left to finish it; behind turn claims the reaper ended
//! it `failed` a claim time to live later, and without them nothing did.
//! Now the processor keeps the event and, once the queue has closed, folds it
//! again, backing off, until the store rules on it: stored, refused because
//! another writer finished the task, or the claim lost. Only an ending whose
//! write gave up on a store that stayed unavailable is kept
//! ([`noting_unavailable`]): one the store answered with a refusal has been
//! ruled on, and writing it again would only spin. When the handler
//! begins to shut down it tries for one more budget and then leaves the
//! turn's claim for the reaper ([`note_ending_owed`]), so a process that
//! stops still leaves a task that something will end.
//!
//! Each attempt is the whole path of a terminal event ([`Processor::handle`]):
//! its push rows reserved, the fold, the log record after the verdict, the
//! push. A fold that fails reverts what it changed in memory, so running it
//! again is the same write, and the store's terminal guard makes a second
//! landing of the same ending a repeat, not a conflict.
//!
//! Cancellation: the loop holds no lock across an await, and every write it
//! makes is one the store either took whole or did not; dropping the future
//! between attempts leaves the store as the last attempt left it.
//!
//! [`note_ending_owed`]: crate::handler::event_processing::note_ending_owed
//! [`noting_unavailable`]: crate::handler::event_processing::noting_unavailable

use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::super::state_machine::{BackgroundDeps, Outcome};
use super::super::{Reservation, record_event};
use super::Processor;
use crate::streaming::StreamEvent;

/// The first wait before an ending is written again.
const FIRST_WAIT: Duration = Duration::from_millis(50);
/// The longest wait between two attempts: one log line, and one write, every
/// this long for a store that keeps refusing.
const MAX_WAIT: Duration = Duration::from_secs(30);

/// A terminal event the store did not confirm, as it came off the queue.
pub(super) struct PendingEnding {
    event: StreamResponse,
    seq: Option<u64>,
    folded_message: bool,
}

impl PendingEnding {
    /// What [`write_pending_ending`](Processor::write_pending_ending) writes
    /// again of `event`, whose terminal status is `ending`, should the store
    /// not confirm it.
    pub(super) fn of(
        ending: &StreamResponse,
        seq: Option<u64>,
        event: &A2aResult<StreamEvent>,
    ) -> Self {
        Self {
            event: ending.clone(),
            seq,
            folded_message: event.as_ref().is_ok_and(|e| e.folded_message),
        }
    }
}

/// Runs `folded`, in `reservation`'s scope when there is one, and returns
/// with its outcome whether a write in it gave up on a store that stayed
/// unavailable (`noting_unavailable`).
pub(super) async fn fold_noting(
    folded: impl std::future::Future<Output = Outcome>,
    reservation: Option<&Reservation>,
) -> (Outcome, bool) {
    crate::handler::event_processing::noting_unavailable(Box::pin(async {
        match reservation {
            // Boxed: the scope would otherwise hold the whole fold inline, in
            // a future that is moved at every poll wrapper above it.
            Some(reservation) => Box::pin(reservation.scope(folded)).await,
            None => folded.await,
        }
    }))
    .await
}

impl<'a> Processor<'a> {
    /// Writes the pending ending again until the store rules on it, or, once
    /// shutdown has begun and one more `store_write_retry` has passed, leaves
    /// the claim for the reaper. See the module documentation.
    pub(super) async fn write_pending_ending(&mut self) {
        let mut wait = FIRST_WAIT;
        let mut cutoff: Option<Instant> = None;
        while let Some(pending) = self.pending_ending.take() {
            if cutoff.is_none() && self.shutdown.is_cancelled() {
                cutoff = Some(Instant::now() + self.deps.limits.store_write_retry);
            }
            if cutoff.is_some_and(|cutoff| Instant::now() >= cutoff) {
                trace_error!(
                    task_id = %self.task_id,
                    "background processor: the store did not take the task's ending before shutdown; \
                     the turn's claim is left for the reaper"
                );
                crate::handler::event_processing::note_ending_owed();
                return;
            }
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = self.shutdown.cancelled(), if cutoff.is_none() => {}
            }
            wait = (wait * 2).min(MAX_WAIT);
            let PendingEnding {
                event,
                seq,
                folded_message,
            } = pending;
            // Boxed: `handle` inline would make every close-out's future
            // as large as the fold's.
            Box::pin(self.handle(Ok(StreamEvent {
                seq,
                event,
                folded_message,
                charge: None,
            })))
            .await;
        }
    }

    /// Watches the handler's `shutdown` token, and folds under `down_limits`
    /// (the handler's limits with no write retried) while the breaker is
    /// open. A processor made without this never opens the breaker and
    /// retries a pending ending until the store rules on it.
    pub(in super::super) fn watching(
        mut self,
        shutdown: CancellationToken,
        down_limits: &'a crate::handler::HandlerLimits,
    ) -> Self {
        self.shutdown = shutdown;
        self.down_limits = Some(down_limits);
        self
    }

    /// Records `event` in the log at `seq`.
    pub(super) async fn record(&self, seq: Option<u64>, event: StreamResponse) {
        if let Some(seq) = seq {
            self.log(&Ok(StreamEvent::at(seq, event))).await;
        }
    }

    /// Records `event` in the log at the position it carries, if any;
    /// `false` when the store was unavailable for the whole budget.
    pub(super) async fn log(&self, event: &A2aResult<StreamEvent>) -> bool {
        record_event(
            self.deps.task_store,
            self.task_id,
            event,
            self.deps.metrics,
            self.fold_deps().limits.store_write_retry,
        )
        .await
    }

    /// The dependencies a write runs under: the handler's, or, while the
    /// breaker is open, its limits with no write retried.
    pub(super) const fn fold_deps(&self) -> BackgroundDeps<'a> {
        match self.down_limits {
            Some(limits) if self.store_down => BackgroundDeps {
                limits,
                ..self.deps
            },
            _ => self.deps,
        }
    }
}
