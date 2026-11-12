// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Settling push reservations a stopped replica left (ADR 0037, audit H14): which
//! of them were stored, from what the task store holds, and arming or removing
//! them. The outbox worker looks every [`SETTLE_EVERY`]; the reaper settles one
//! task's at once (`PushLanes::settle_task`).

use std::time::Duration;

use a2a_protocol_types::task::TaskId;

use super::lanes::Lanes;
use super::outbox_worker::{BATCH, Outboxed};
use super::reservation::Verdict;
use crate::metrics::push_outcome;
use crate::push::outbox::Held;
use crate::store::tenant::TenantContext;

/// How often the worker looks for reservations to settle.
pub const SETTLE_EVERY: Duration = Duration::from_secs(10);

/// The most batches of [`BATCH`] reservations one look settles.
const MAX_ROUNDS: usize = 64;

/// Settles the reservations whose hold has run out: their reserver stopped (or
/// is stalled past any fold), so the task store says whether the event was
/// stored (`reservation::verdict`), and the rows are armed or removed. Every
/// replica does this; arming and releasing are idempotent, and a reservation
/// another replica settled first is simply gone.
pub(super) async fn settle_held(lanes: &Lanes, outboxed: &Outboxed) {
    let (Some(tasks), true) = (
        lanes.tasks.as_ref(),
        outboxed.outbox.supports_reservations(),
    ) else {
        return;
    };
    // A replica lost with many tasks in flight leaves that many reservations,
    // and one batch every `SETTLE_EVERY` would take minutes: go on while a
    // batch comes back full and settled something. A reservation that cannot
    // be settled (a task store that cannot be read) is listed again, so a
    // round that settled nothing ends the look.
    for _ in 0..MAX_ROUNDS {
        let held = match outboxed.outbox.held(BATCH).await {
            Ok(held) => held,
            Err(_err) => {
                trace_warn!(error = %_err, "the push outbox's reservations could not be read; trying again");
                return;
            }
        };
        let listed = held.len();
        let mut any_settled = false;
        for held in held {
            let tenant = held.tenant.clone();
            let caller = held.caller.clone();
            let settle = async {
                if let Some(caller) = caller {
                    // A fresh request scope, so this cannot conflict.
                    let _ = TenantContext::set_caller(&caller);
                }
                settle_one(lanes, outboxed, tasks.as_ref(), held).await
            };
            any_settled |= TenantContext::scope_request(tenant, settle).await;
        }
        if listed < BATCH || !any_settled {
            return;
        }
    }
}

/// One held reservation, settled by what the task store holds: whether it was
/// settled. A store that cannot be read leaves it for the next round.
pub(super) async fn settle_one(
    lanes: &Lanes,
    outboxed: &Outboxed,
    tasks: &dyn crate::store::TaskStore,
    held: Held,
) -> bool {
    let task_id = TaskId::new(held.task_id.clone());
    let Some(verdict) = verdict_of(tasks, &task_id, &held).await else {
        return false;
    };
    let settled = match verdict {
        Verdict::Arm => outboxed.outbox.arm(&held.reservation).await.map(|_| ()),
        Verdict::Release => outboxed.outbox.release(&held.reservation).await,
    };
    match settled {
        Ok(()) => {
            trace_warn!(
                task_id = %task_id,
                log_seq = held.log_seq,
                "settled a push reservation its replica left behind"
            );
            lanes
                .metrics
                .on_push_delivery(push_outcome::RESERVATION_SETTLED);
            outboxed.wake.notify_one();
            true
        }
        Err(_err) => {
            trace_warn!(task_id = %task_id, error = %_err, "a held push reservation could not be settled; trying again");
            false
        }
    }
}

/// Whether `held`'s event was stored; `None` when the task store cannot say now.
async fn verdict_of(
    tasks: &dyn crate::store::TaskStore,
    task_id: &TaskId,
    held: &Held,
) -> Option<Verdict> {
    verdict_for(tasks, task_id, held.log_seq, &held.event).await
}

/// Whether the event reserved at `log_seq` was stored, from what the task
/// store holds; `None` when it cannot say now.
pub(super) async fn verdict_for(
    tasks: &dyn crate::store::TaskStore,
    task_id: &TaskId,
    log_seq: u64,
    event: &a2a_protocol_types::events::StreamResponse,
) -> Option<Verdict> {
    if log_seq == super::reservation::ENDING {
        // A task's ending: stored if the task is in the terminal state it names.
        return match tasks.get(task_id).await {
            Ok(task) => Some(super::reservation::ending_verdict(
                event,
                task.map(|task| task.status.state),
            )),
            Err(_err) => {
                trace_warn!(task_id = %task_id, error = %_err, "a held push reservation could not be checked against its task; trying again");
                None
            }
        };
    }
    let position = match tasks.get_with_position(task_id).await {
        Ok(found) => found.and_then(|(_, position)| position),
        Err(_err) => {
            trace_warn!(task_id = %task_id, error = %_err, "a held push reservation could not be checked against its task; trying again");
            return None;
        }
    };
    // The log is read only when the task reflects the position: a task that
    // has not reached it did not store the event, whatever the log holds.
    let logged = if position.is_some_and(|stored| stored >= log_seq) {
        match tasks
            .read_events(task_id, log_seq.saturating_sub(1), 1)
            .await
        {
            Ok(events) => events
                .into_iter()
                .next()
                .filter(|recorded| recorded.seq == log_seq)
                .map(|recorded| recorded.event),
            Err(_err) => {
                trace_warn!(task_id = %task_id, error = %_err, "a held push reservation's event could not be looked up in its log; trying again");
                return None;
            }
        }
    } else {
        None
    };
    Some(super::reservation::verdict(
        event,
        log_seq,
        position,
        logged.as_ref(),
    ))
}
