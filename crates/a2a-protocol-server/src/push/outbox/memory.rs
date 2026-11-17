// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`PushOutbox`] in memory: the outbox's rules for a process whose other
//! stores are in memory too. Nothing survives a restart.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

use super::{ClaimedDelivery, Held, NewDelivery, OutboxFuture, PushOutbox};

/// The webhook of a task a row is for: its rows are delivered in order.
type Webhook = (String, Option<String>, String, String);

fn webhook(d: &NewDelivery) -> Webhook {
    (
        d.tenant.clone(),
        d.caller.clone(),
        d.task_id.clone(),
        d.config_id.clone(),
    )
}

struct Row {
    delivery: NewDelivery,
    recorded: Instant,
    due: Instant,
    attempts: u32,
    /// The owner and the end of its lease, while claimed.
    lease: Option<(String, Instant)>,
    /// Reserved before its event was stored; armed once it was.
    reservation: Option<Reserved>,
}

/// What a row remembers of its reservation.
struct Reserved {
    id: String,
    log_seq: u64,
    /// After this the reserver is presumed stopped.
    hold_until: Instant,
    /// The event was stored: the row can be claimed.
    armed: bool,
}

impl Row {
    fn leased(&self, now: Instant) -> bool {
        self.lease.as_ref().is_some_and(|(_, until)| *until > now)
    }

    /// Reserved and not armed yet.
    fn unarmed(&self) -> bool {
        self.reservation.as_ref().is_some_and(|r| !r.armed)
    }
}

#[derive(Default)]
struct State {
    next: i64,
    rows: BTreeMap<i64, Row>,
}

/// An in-memory [`PushOutbox`].
#[derive(Default)]
pub struct InMemoryPushOutbox {
    state: Mutex<State>,
}

impl std::fmt::Debug for InMemoryPushOutbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryPushOutbox")
            .field("pending", &self.lock().rows.len())
            .finish()
    }
}

impl InMemoryPushOutbox {
    /// An empty outbox.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    /// Makes room for one more row of `key` under `per_webhook`; whether the
    /// new row, which ends a turn when `ends_turn`, is to be kept.
    fn make_room(
        &mut self,
        key: &Webhook,
        ends_turn: bool,
        per_webhook: usize,
        now: Instant,
    ) -> (bool, usize) {
        let waiting: Vec<i64> = self
            .rows
            .iter()
            .filter(|(_, row)| webhook(&row.delivery) == *key)
            .map(|(seq, _)| *seq)
            .collect();
        if waiting.len() < per_webhook.max(1) {
            return (true, 0);
        }
        let oldest = waiting.into_iter().find(|seq| {
            self.rows
                .get(seq)
                .is_some_and(|row| !row.delivery.ends_turn && !row.leased(now) && !row.unarmed())
        });
        match oldest {
            Some(seq) => {
                self.rows.remove(&seq);
                (true, 1)
            }
            None if !ends_turn => (false, 1),
            None => (true, 0),
        }
    }
}

impl InMemoryPushOutbox {
    /// Inserts `deliveries` in order, each `reserved` as given; how many
    /// were dropped for the bound.
    fn insert(
        &self,
        deliveries: Vec<NewDelivery>,
        per_webhook: usize,
        reserved: impl Fn() -> Option<Reserved>,
    ) -> usize {
        let now = Instant::now();
        let mut state = self.lock();
        let mut dropped = 0;
        for delivery in deliveries {
            // Recorded already (an id is a row's identity): left as it is.
            if state
                .rows
                .values()
                .any(|row| row.delivery.delivery_id == delivery.delivery_id)
            {
                continue;
            }
            let (keep, made_room) =
                state.make_room(&webhook(&delivery), delivery.ends_turn, per_webhook, now);
            dropped += made_room;
            if keep {
                state.next += 1;
                let seq = state.next;
                state.rows.insert(
                    seq,
                    Row {
                        delivery,
                        recorded: now,
                        due: now,
                        attempts: 0,
                        lease: None,
                        reservation: reserved(),
                    },
                );
            }
        }
        drop(state);
        dropped
    }
}

impl PushOutbox for InMemoryPushOutbox {
    fn is_process_local(&self) -> bool {
        true
    }

    fn record(&self, deliveries: Vec<NewDelivery>, per_webhook: usize) -> OutboxFuture<'_, usize> {
        Box::pin(async move { Ok(self.insert(deliveries, per_webhook, || None)) })
    }

    fn supports_reservations(&self) -> bool {
        true
    }

    fn reserve<'a>(
        &'a self,
        reservation: &'a str,
        log_seq: u64,
        deliveries: Vec<NewDelivery>,
        per_webhook: usize,
        hold: Duration,
    ) -> OutboxFuture<'a, usize> {
        Box::pin(async move {
            let now = Instant::now();
            let hold_until = now.checked_add(hold).unwrap_or(now);
            Ok(self.insert(deliveries, per_webhook, || {
                Some(Reserved {
                    id: reservation.to_owned(),
                    log_seq,
                    hold_until,
                    armed: false,
                })
            }))
        })
    }

    fn arm<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, usize> {
        Box::pin(async move {
            let mut state = self.lock();
            let mut found = 0;
            for row in state.rows.values_mut() {
                if let Some(held) = row.reservation.as_mut()
                    && held.id == reservation
                {
                    held.armed = true;
                    found += 1;
                }
            }
            drop(state);
            Ok(found)
        })
    }

    fn extend_hold<'a>(&'a self, reservation: &'a str, hold: Duration) -> OutboxFuture<'a, usize> {
        Box::pin(async move {
            let now = Instant::now();
            let until = now.checked_add(hold).unwrap_or(now);
            let mut state = self.lock();
            let mut extended = 0;
            for row in state.rows.values_mut() {
                if let Some(held) = row.reservation.as_mut()
                    && held.id == reservation
                    && !held.armed
                {
                    held.hold_until = until;
                    extended += 1;
                }
            }
            drop(state);
            Ok(extended)
        })
    }

    fn release<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, ()> {
        Box::pin(async move {
            self.lock().rows.retain(|_, row| {
                row.reservation
                    .as_ref()
                    .is_none_or(|held| held.armed || held.id != reservation)
            });
            Ok(())
        })
    }

    fn held_for_task<'a>(
        &'a self,
        tenant: &'a str,
        caller: Option<&'a str>,
        task_id: &'a str,
    ) -> OutboxFuture<'a, Vec<Held>> {
        Box::pin(async move {
            let state = self.lock();
            let mut seen = HashSet::new();
            let mut held = Vec::new();
            for row in state.rows.values() {
                let Some(reserved) = row.reservation.as_ref().filter(|r| !r.armed) else {
                    continue;
                };
                let d = &row.delivery;
                if d.tenant == tenant
                    && d.caller.as_deref() == caller
                    && d.task_id == task_id
                    && seen.insert(reserved.id.clone())
                {
                    held.push(Held {
                        reservation: reserved.id.clone(),
                        tenant: d.tenant.clone(),
                        caller: d.caller.clone(),
                        task_id: d.task_id.clone(),
                        log_seq: reserved.log_seq,
                        event: d.event.clone(),
                    });
                }
            }
            drop(state);
            Ok(held)
        })
    }

    fn supports_renewal(&self) -> bool {
        true
    }

    fn renew<'a>(&'a self, owner: &'a str, lease: Duration) -> OutboxFuture<'a, usize> {
        Box::pin(async move {
            let now = Instant::now();
            let until = now.checked_add(lease).unwrap_or(now);
            let mut state = self.lock();
            let mut renewed = 0;
            for row in state.rows.values_mut() {
                if row.leased(now)
                    && let Some((held, end)) = row.lease.as_mut()
                    && held == owner
                {
                    *end = until;
                    renewed += 1;
                }
            }
            drop(state);
            Ok(renewed)
        })
    }

    fn held(&self, limit: usize) -> OutboxFuture<'_, Vec<Held>> {
        Box::pin(async move {
            let now = Instant::now();
            let state = self.lock();
            let mut seen = HashSet::new();
            let mut held = Vec::new();
            for row in state.rows.values() {
                let Some(reserved) = row.reservation.as_ref().filter(|r| !r.armed) else {
                    continue;
                };
                if held.len() >= limit || reserved.hold_until > now {
                    continue;
                }
                if seen.insert(reserved.id.clone()) {
                    held.push(Held {
                        reservation: reserved.id.clone(),
                        tenant: row.delivery.tenant.clone(),
                        caller: row.delivery.caller.clone(),
                        task_id: row.delivery.task_id.clone(),
                        log_seq: reserved.log_seq,
                        event: row.delivery.event.clone(),
                    });
                }
            }
            drop(state);
            Ok(held)
        })
    }

    fn claim<'a>(
        &'a self,
        owner: &'a str,
        max: usize,
        lease: Duration,
    ) -> OutboxFuture<'a, Vec<ClaimedDelivery>> {
        Box::pin(async move {
            let now = Instant::now();
            let until = now.checked_add(lease).unwrap_or(now);
            let mut state = self.lock();
            let mut earlier: HashSet<Webhook> = HashSet::new();
            let mut claimed = Vec::new();
            for (seq, row) in &mut state.rows {
                if claimed.len() >= max {
                    break;
                }
                // The first row of each webhook is the only one that can be
                // claimed; a later one waits for it.
                if !earlier.insert(webhook(&row.delivery)) {
                    continue;
                }
                // A reserved row holds the place and is not claimed.
                if row.due > now || row.leased(now) || row.unarmed() {
                    continue;
                }
                row.lease = Some((owner.to_owned(), until));
                claimed.push(ClaimedDelivery {
                    seq: *seq,
                    delivery: row.delivery.clone(),
                    attempts: row.attempts,
                    age: now.saturating_duration_since(row.recorded),
                });
            }
            drop(state);
            Ok(claimed)
        })
    }

    fn complete(&self, seq: i64) -> OutboxFuture<'_, ()> {
        Box::pin(async move {
            self.lock().rows.remove(&seq);
            Ok(())
        })
    }

    fn retry_after<'a>(
        &'a self,
        seq: i64,
        owner: &'a str,
        after: Duration,
    ) -> OutboxFuture<'a, ()> {
        Box::pin(async move {
            let now = Instant::now();
            let mut state = self.lock();
            if let Some(row) = state.rows.get_mut(&seq)
                && row.lease.as_ref().is_some_and(|(held, _)| held == owner)
            {
                row.attempts = row.attempts.saturating_add(1);
                row.due = now.checked_add(after).unwrap_or(now);
                row.lease = None;
            }
            drop(state);
            Ok(())
        })
    }

    fn pending(&self) -> OutboxFuture<'_, usize> {
        Box::pin(async move { Ok(self.lock().rows.len()) })
    }
}
