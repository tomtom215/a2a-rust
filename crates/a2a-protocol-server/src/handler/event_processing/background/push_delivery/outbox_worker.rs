// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The push lanes' half of the durable outbox (ADR 0037): what a lane does
//! with an event when the handler has a [`PushOutbox`], and the worker that
//! delivers what the outbox holds.
//!
//! A lane records a stored event's deliveries, one row per config, in the
//! order the events were stored, and wakes the worker. The worker claims
//! rows, delivers each through the handler's sender (the gate, the health
//! watch, the sender), and ends the row (`2xx`, a refusal no retry will
//! change, a config that is gone, a row past `push_outbox_max_age`) or makes
//! it due again after a backoff. It stops at shutdown and when the handler's
//! lanes are gone; what it held is released, and what it could not release
//! is claimed again once its lease runs out.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::TaskId;
use futures_util::StreamExt as _;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::lanes::{Lanes, ends_turn};
use super::reservation::Verdict;
use crate::metrics::push_outcome;
use crate::push::outbox::{self, ClaimedDelivery, Held, NewDelivery, PushOutbox};
use crate::store::tenant::TenantContext;

/// The most rows one claim takes.
pub const BATCH: usize = 32;

/// How often an idle worker looks for rows another replica recorded, or that
/// came due.
pub const POLL: Duration = Duration::from_secs(1);

/// The handler's outbox and its worker's identity.
pub struct Outboxed {
    pub outbox: Arc<dyn PushOutbox>,
    /// Woken when this replica records a row.
    pub wake: Notify,
    /// The owner its claims are leased to: one per handler.
    pub owner: String,
}

impl Outboxed {
    pub fn new(outbox: Arc<dyn PushOutbox>) -> Self {
        Self {
            outbox,
            wake: Notify::new(),
            owner: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// How long a claim is leased when the outbox renews leases: a worker that
/// stops renewing (it died, or stalled) loses its rows after this, not after
/// the longest a delivery can take.
pub const RENEWED_LEASE: Duration = Duration::from_secs(30);

/// How often a worker renews what it holds: a third of [`RENEWED_LEASE`], so
/// two renewals can be missed.
pub const RENEW_EVERY: Duration = Duration::from_secs(10);

/// The lease for a claim: short with renewal, and otherwise longer than one
/// delivery can take.
pub fn claim_lease(outbox: &dyn PushOutbox, push_delivery_timeout: Duration) -> Duration {
    if outbox.supports_renewal() {
        RENEWED_LEASE
    } else {
        lease(push_delivery_timeout)
    }
}

/// How long a claim is leased: longer than one delivery can take (a wait for
/// the gate, then the webhook's time, then a wait for an address, then its
/// time again), so a lease that runs out belonged to a replica that stopped.
pub const fn lease(push_delivery_timeout: Duration) -> Duration {
    push_delivery_timeout
        .saturating_mul(3)
        .saturating_add(Duration::from_secs(30))
}

/// How long a reservation is held before the worker settles it: longer than
/// a fold can take, however long the task store makes it retry (the log
/// write, then the save, each for up to `store_write_retry`), so that a hold
/// that runs out belonged to a replica that stopped.
pub const fn hold(limits: &crate::handler::limits::HandlerLimits) -> Duration {
    limits
        .store_write_retry
        .saturating_mul(3)
        .saturating_add(Duration::from_secs(60))
}

/// How often the worker looks for reservations to settle.
pub const SETTLE_EVERY: Duration = Duration::from_secs(10);

/// The most batches of [`BATCH`] reservations one look settles.
const MAX_ROUNDS: usize = 64;

/// Records `events`' deliveries to every config of `task_id`, in the caller's
/// tenant. A store that will not take them costs nothing more than the lanes
/// did: the deliveries are made from memory, now.
pub async fn record(
    lanes: &Lanes,
    outboxed: &Outboxed,
    task_id: &TaskId,
    events: &[StreamResponse],
) {
    record_with_ids(lanes, outboxed, task_id, events, &[]).await;
}

/// Records `event` again under the delivery ids its rows had: the reservation
/// that held them was settled before the event was stored. Recording is
/// idempotent on a delivery id, so rows that are still there stay one row.
pub async fn record_again(
    lanes: &Lanes,
    outboxed: &Outboxed,
    task_id: &TaskId,
    event: &StreamResponse,
    ids: &[(String, String)],
) {
    record_with_ids(lanes, outboxed, task_id, std::slice::from_ref(event), ids).await;
}

/// [`record`], with the delivery id of each `(config id, delivery id)` in `ids`.
async fn record_with_ids(
    lanes: &Lanes,
    outboxed: &Outboxed,
    task_id: &TaskId,
    events: &[StreamResponse],
    ids: &[(String, String)],
) {
    let Some(configs) =
        super::list_configs(task_id, &*lanes.store, &lanes.limits, &*lanes.metrics).await
    else {
        return;
    };
    let rows = rows_with_ids(task_id, &configs, events, ids);
    if rows.is_empty() {
        return;
    }
    match outboxed
        .outbox
        .record(rows, lanes.limits.push_queue_capacity)
        .await
    {
        Ok(dropped) => {
            for _ in 0..dropped {
                lanes.metrics.on_push_delivery(push_outcome::DROPPED);
            }
            outboxed.wake.notify_one();
        }
        Err(_err) => {
            trace_warn!(
                task_id = %task_id,
                error = %_err,
                "the push outbox did not take these deliveries; delivering them from memory"
            );
            let timeout_label = super::timeout_outcome(&*lanes.sender, &lanes.limits);
            for event in events {
                super::deliver_to_every_config(
                    task_id,
                    &*lanes.sender,
                    &configs,
                    event,
                    lanes.limits.push_delivery_timeout,
                    configs.len(),
                    &*lanes.metrics,
                    timeout_label,
                )
                .await;
            }
        }
    }
}

/// One row per event and config, in order, in the caller's tenant and caller.
pub(super) fn rows(
    task_id: &TaskId,
    configs: &[a2a_protocol_types::push::TaskPushNotificationConfig],
    events: &[StreamResponse],
) -> Vec<NewDelivery> {
    rows_with_ids(task_id, configs, events, &[])
}

/// [`rows`], a config in `ids` keeping the delivery id it names. For one event.
fn rows_with_ids(
    task_id: &TaskId,
    configs: &[a2a_protocol_types::push::TaskPushNotificationConfig],
    events: &[StreamResponse],
    ids: &[(String, String)],
) -> Vec<NewDelivery> {
    let (tenant, caller) = (TenantContext::current(), TenantContext::caller());
    let mut rows = Vec::with_capacity(events.len() * configs.len());
    for event in events {
        // Every store gives a config an id when it is set.
        for config_id in configs.iter().filter_map(|c| c.id.clone()) {
            let delivery_id = ids
                .iter()
                .find(|(config, _)| *config == config_id)
                .map_or_else(|| uuid::Uuid::new_v4().to_string(), |(_, id)| id.clone());
            rows.push(NewDelivery {
                delivery_id,
                tenant: tenant.clone(),
                caller: caller.clone(),
                task_id: task_id.as_ref().to_owned(),
                config_id,
                event: event.clone(),
                ends_turn: ends_turn(event),
            });
        }
    }
    rows
}

/// Delivers what the outbox holds until `shutdown`, or until the lanes are
/// gone.
pub async fn run(lanes: Weak<Lanes>, shutdown: CancellationToken) {
    let mut next_settle = tokio::time::Instant::now();
    loop {
        let Some(strong) = lanes.upgrade() else {
            return;
        };
        let Some(outboxed) = strong.outbox.as_ref() else {
            return;
        };
        if tokio::time::Instant::now() >= next_settle {
            next_settle = tokio::time::Instant::now() + SETTLE_EVERY;
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = settle_held(&strong, outboxed) => {}
            }
        }
        let lease = claim_lease(&*outboxed.outbox, strong.limits.push_delivery_timeout);
        let claimed = tokio::select! {
            () = shutdown.cancelled() => return,
            claimed = outboxed.outbox.claim(&outboxed.owner, BATCH, lease) => claimed,
        };
        let claimed = claimed.unwrap_or_else(|_err| {
            trace_warn!(error = %_err, "the push outbox could not be read; trying again");
            Vec::new()
        });
        if claimed.is_empty() {
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = outboxed.wake.notified() => {}
                () = tokio::time::sleep(POLL) => {}
            }
            continue;
        }
        if !deliver_batch(&strong, outboxed, claimed, &shutdown, lease).await {
            return;
        }
    }
}

/// Settles the reservations whose hold has run out: their reserver stopped (or
/// is stalled past any fold), so the task store says whether the event was
/// stored (`reservation::verdict`), and the rows are armed or removed. Every
/// replica does this; arming and releasing are idempotent, and a reservation
/// another replica settled first is simply gone.
async fn settle_held(lanes: &Lanes, outboxed: &Outboxed) {
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
        let mut settled = 0;
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
            if TenantContext::scope_request(tenant, settle).await {
                settled += 1;
            }
        }
        if listed < BATCH || settled == 0 {
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
    if held.log_seq == super::reservation::ENDING {
        // A task's ending: stored if the task is in the terminal state it names.
        return match tasks.get(task_id).await {
            Ok(task) => Some(super::reservation::ending_verdict(
                &held.event,
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
    let logged = if position.is_some_and(|stored| stored >= held.log_seq) {
        match tasks
            .read_events(task_id, held.log_seq.saturating_sub(1), 1)
            .await
        {
            Ok(events) => events
                .into_iter()
                .next()
                .filter(|recorded| recorded.seq == held.log_seq)
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
        &held.event,
        held.log_seq,
        position,
        logged.as_ref(),
    ))
}

/// Delivers `claimed`, at once (the gate bounds them); `false` when shutdown
/// cut it short, after releasing the rows still running.
async fn deliver_batch(
    lanes: &Lanes,
    outboxed: &Outboxed,
    claimed: Vec<ClaimedDelivery>,
    shutdown: &CancellationToken,
    lease: Duration,
) -> bool {
    let running = Mutex::new(claimed.iter().map(|c| c.seq).collect::<HashSet<i64>>());
    let mut deliveries: futures_util::stream::FuturesUnordered<_> = claimed
        .into_iter()
        .map(|c| deliver_claimed(lanes, outboxed, c))
        .collect();
    // The heartbeat: while deliveries run, the lease on what this worker holds
    // is renewed, so a lease that runs out is one whose worker is gone.
    let heartbeat = outboxed.outbox.supports_renewal();
    let mut renewals = tokio::time::interval(RENEW_EVERY);
    renewals.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    renewals.tick().await;
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = renewals.tick(), if heartbeat => {
                if let Err(_err) = outboxed.outbox.renew(&outboxed.owner, lease).await {
                    trace_warn!(error = %_err, "the push outbox did not renew this worker's leases");
                }
            }
            done = deliveries.next() => match done {
                Some(seq) => {
                    running.lock().unwrap_or_else(PoisonError::into_inner).remove(&seq);
                }
                None => return true,
            },
        }
    }
    drop(deliveries);
    let left: Vec<i64> = running
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .into_iter()
        .collect();
    for seq in left {
        let _ = outboxed
            .outbox
            .retry_after(seq, &outboxed.owner, Duration::ZERO)
            .await;
    }
    false
}

/// One claimed row, delivered in the tenant and caller it was recorded in;
/// its `seq`.
async fn deliver_claimed(lanes: &Lanes, outboxed: &Outboxed, claimed: ClaimedDelivery) -> i64 {
    let seq = claimed.seq;
    let tenant = claimed.delivery.tenant.clone();
    let caller = claimed.delivery.caller.clone();
    let delivery = async move {
        if let Some(caller) = caller {
            // A fresh request scope, so this cannot conflict.
            let _ = TenantContext::set_caller(&caller);
        }
        deliver_in_scope(lanes, outboxed, claimed).await;
    };
    crate::rpc_span::in_child_span(
        "a2a.deliver_push",
        TenantContext::scope_request(tenant, delivery),
    )
    .await;
    seq
}

async fn deliver_in_scope(lanes: &Lanes, outboxed: &Outboxed, claimed: ClaimedDelivery) {
    let ClaimedDelivery {
        seq,
        delivery,
        attempts,
        age,
    } = claimed;
    let outbox = &outboxed.outbox;
    if age >= lanes.limits.push_outbox_max_age {
        trace_warn!(
            task_id = %delivery.task_id,
            attempts,
            "push delivery expired: older than push_outbox_max_age, not delivered"
        );
        lanes.metrics.on_push_delivery(push_outcome::EXPIRED);
        finish(outbox.complete(seq).await);
        return;
    }
    let config = match lanes
        .store
        .get(&delivery.task_id, &delivery.config_id)
        .await
    {
        Ok(Some(config)) => config,
        // Deleted, or removed after a `410`: nothing to deliver to.
        Ok(None) => {
            finish(outbox.complete(seq).await);
            return;
        }
        Err(_err) => {
            trace_warn!(task_id = %delivery.task_id, error = %_err, "a push config could not be read; the delivery waits");
            lanes.metrics.on_push_delivery(push_outcome::FAILED);
            later(lanes, outboxed, seq, attempts).await;
            return;
        }
    };
    let task_id = TaskId::new(delivery.task_id.clone());
    let timeout_label = super::timeout_outcome(&*lanes.sender, &lanes.limits);
    let attempted = crate::push::with_delivery_id(
        delivery.delivery_id.clone(),
        super::attempt(
            &task_id,
            &*lanes.sender,
            &config,
            &delivery.event,
            lanes.limits.push_delivery_timeout,
            timeout_label,
        ),
    )
    .await;
    lanes.metrics.on_push_delivery(attempted.outcome);
    if attempted.settled {
        finish(outbox.complete(seq).await);
    } else {
        later(lanes, outboxed, seq, attempts).await;
    }
}

/// Makes row `seq` due again after its backoff.
async fn later(lanes: &Lanes, outboxed: &Outboxed, seq: i64, attempts: u32) {
    let after = outbox::backoff(
        attempts.saturating_add(1),
        lanes.limits.push_outbox_backoff_max,
        outbox::jitter(),
    );
    finish(
        outboxed
            .outbox
            .retry_after(seq, &outboxed.owner, after)
            .await,
    );
}

/// A row the outbox could not end or release is claimed again once its lease
/// runs out, and delivered again: at least once.
fn finish(result: a2a_protocol_types::error::A2aResult<()>) {
    if let Err(_err) = result {
        trace_warn!(error = %_err, "the push outbox could not be updated; the row is claimed again after its lease");
    }
}
