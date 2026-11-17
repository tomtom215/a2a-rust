// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How the processor tells a task's ending that no queued event carried: to
//! its webhooks first, durably, and then to its log; and what it leaves owed
//! when it cannot (adversarial review N06, 2026-11-13).

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::TaskStatus;

use super::Processor;

impl Processor<'_> {
    /// Tells the task's webhooks how it ended, unless this processor already
    /// did, and records the ending in its log at the next position.
    pub(super) async fn announce(
        &mut self,
        status: TaskStatus,
        reserved: Option<super::super::Reservation>,
    ) {
        let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
            self.task_id.clone(),
            self.last_task.context_id.clone(),
            status,
        ));
        // Recorded in the outbox before it is logged, when there is one (see
        // `PushLanes::record_now`): a stop in between leaves the log without
        // the ending, which the reaper completes, not a log with it and
        // webhooks that were never told.
        let (recorded, durable) = match reserved {
            // Reserved before the status was saved: armed now that it is stored.
            Some(reservation) if !self.pushed_terminal => (true, reservation.arm_now().await),
            Some(reservation) => {
                reservation.release_now().await;
                (true, true)
            }
            None if self.pushed_terminal => (false, true),
            None => self.record_ending(&event).await,
        };
        // Not queued durably: the ending stays out of the log, which is what
        // the reaper looks for, and the claim is kept for it (N06).
        if durable {
            crate::handler::event_processing::append_ending(
                self.deps.task_store,
                self.task_id,
                &event,
                self.deps.metrics,
                self.deps.limits.store_write_retry,
            )
            .await;
        } else {
            crate::handler::event_processing::note_ending_owed();
        }
        if !self.pushed_terminal {
            if !recorded {
                self.deps.push.push(self.task_id, &event);
            }
            self.pushed_terminal = true;
        }
        self.ended = true;
    }

    /// Records the ending's deliveries in the outbox: whether the outbox took
    /// them at all (`false`: there is none, and the caller pushes the event
    /// itself), and whether they are durable, or none was owed (N06).
    pub(super) async fn record_ending(&self, event: &StreamResponse) -> (bool, bool) {
        self.deps
            .push
            .record_now(self.task_id, event)
            .await
            .map_or((false, true), |recorded| (true, recorded.is_durable()))
    }
}
