// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`PushOutbox`] in memory: the outbox's rules for a process whose other
//! stores are in memory too. Nothing survives a restart.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

use super::{ClaimedDelivery, NewDelivery, OutboxFuture, PushOutbox};

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
}

impl Row {
    fn leased(&self, now: Instant) -> bool {
        self.lease.as_ref().is_some_and(|(_, until)| *until > now)
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
                .is_some_and(|row| !row.delivery.ends_turn && !row.leased(now))
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

impl PushOutbox for InMemoryPushOutbox {
    fn record(&self, deliveries: Vec<NewDelivery>, per_webhook: usize) -> OutboxFuture<'_, usize> {
        Box::pin(async move {
            let now = Instant::now();
            let mut state = self.lock();
            let mut dropped = 0;
            for delivery in deliveries {
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
                        },
                    );
                }
            }
            drop(state);
            Ok(dropped)
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
                if row.due > now || row.leased(now) {
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
