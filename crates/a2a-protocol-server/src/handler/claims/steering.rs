// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Steering messages across replicas (ADR 0036): handing one to the replica
//! that holds a task's claim, and moving what other replicas handed this one
//! into the inboxes of the turns it runs.
//!
//! The sender's replica stores the message for the claim
//! ([`TurnClaimStore::hand_steering`]); the claim's holder looks at what is
//! stored for its claims every renewal interval, and at once when the store
//! signals that a message arrived (PostgreSQL, when notifications are on). A
//! message stays stored until it is in the inbox, so a full inbox delays it
//! and never loses it; the store's queue is bounded by the same capacity, so
//! a task holds at most twice its inbox's capacity.

use std::collections::HashSet;
use std::sync::{Arc, Weak};
use std::time::Duration;

use a2a_protocol_types::task::TaskId;
use tokio::sync::broadcast::error::RecvError;

use super::TurnClaims;
use crate::error::ServerResult;
use crate::handler::RequestHandler;
use crate::metrics::Metrics;
use crate::metrics::control::steering as outcome;
use crate::steering::{Inbox, Refused, Steered, SteeringConfig};
use crate::store::claim::Handed;
use crate::store::tenant::TenantContext;

/// The most messages one look at the store reads, over every claim held.
const PEEK_LIMIT: usize = 256;

/// What a handler that takes steering does about its claims' stores.
pub struct SteeringSync {
    /// Whether to listen for the store's signal that a message arrived,
    /// beside looking every renewal interval.
    pub listen: bool,
    /// Where `received` and `unread` are counted.
    pub metrics: Arc<dyn Metrics>,
}

impl RequestHandler {
    /// Stores `steered` for the replica that holds the claim on `task_id`,
    /// in the current tenant and caller partition.
    ///
    /// # Errors
    ///
    /// The message cannot be written, or the store's.
    pub(in crate::handler) async fn hand_steering(
        &self,
        task_id: &TaskId,
        steered: &Steered,
        config: &SteeringConfig,
    ) -> ServerResult<Handed> {
        let Some(claims) = &self.turn_claims else {
            return Ok(Handed::Unsupported);
        };
        let message = serde_json::to_string(steered)?;
        Ok(claims
            .store
            .hand_steering(
                &TenantContext::partition(),
                &task_id.0,
                &message,
                config.inbox_capacity(),
            )
            .await?)
    }
}

impl TurnClaims {
    /// The claims whose turn has an inbox on this replica: token and inbox.
    fn inboxes(&self) -> Vec<(String, Arc<Inbox>)> {
        self.held()
            .iter()
            .filter_map(|(token, held)| {
                let inbox = held.turn.as_ref()?.inbox()?;
                Some((token.clone(), Arc::clone(inbox)))
            })
            .collect()
    }

    /// Moves what the store holds for this replica's claims into their
    /// turns' inboxes, oldest first, and deletes it from the store once it is
    /// there. A message that does not fit stays stored for the next look,
    /// with every later message for the same claim, so order is kept. One
    /// that cannot be read, or whose turn ended, is deleted and not retried.
    /// Failures are logged: the next look tries again.
    pub(super) async fn deliver_steering(&self) {
        let Some(sync) = &self.steering else { return };
        // One look at a time, so a signal and a poll do not both deliver one.
        let _serial = self.steering_look.lock().await;
        let inboxes = self.inboxes();
        if inboxes.is_empty() {
            return;
        }
        let tokens: Vec<String> = inboxes.iter().map(|(token, _)| token.clone()).collect();
        let pending = match self
            .store
            .peek_steering(&self.owner, &tokens, PEEK_LIMIT)
            .await
        {
            Ok(pending) => pending,
            Err(_e) => {
                trace_warn!(error = %_e, "steering messages not read; retrying next interval");
                return;
            }
        };
        let mut done = Vec::new();
        let mut full = HashSet::new();
        for item in pending {
            if full.contains(&item.token) {
                continue;
            }
            let Some((_, inbox)) = inboxes.iter().find(|(token, _)| *token == item.token) else {
                continue;
            };
            match serde_json::from_str::<Steered>(&item.message) {
                Err(_e) => {
                    trace_warn!(error = %_e, "a stored steering message is not one; dropped");
                    done.push(item.seq);
                }
                Ok(steered) => match inbox.try_push(steered) {
                    Ok(()) => {
                        sync.metrics.on_steering(outcome::RECEIVED);
                        done.push(item.seq);
                    }
                    Err(Refused::Full) => {
                        full.insert(item.token);
                    }
                    Err(Refused::Closed) => {
                        sync.metrics.on_steering(outcome::UNREAD);
                        done.push(item.seq);
                    }
                },
            }
        }
        if !done.is_empty()
            && let Err(_e) = self.store.ack_steering(&done).await
        {
            trace_warn!(error = %_e, "delivered steering messages not deleted from the store");
        }
    }
}

/// Looks at the store for steering messages every `every`, and when the
/// store signals one, until the handler is dropped.
pub(super) async fn steering_loop(claims: Weak<TurnClaims>, every: Duration) {
    let mut signals = claims
        .upgrade()
        .filter(|claims| claims.steering.as_ref().is_some_and(|s| s.listen))
        .and_then(|claims| claims.store.steering_signals());
    let mut tick = tokio::time::interval(every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        match signals.as_mut() {
            Some(rx) => tokio::select! {
                _ = tick.tick() => {}
                signal = rx.recv() => {
                    // A burst the channel dropped is looked at all the same.
                    if matches!(signal, Err(RecvError::Closed)) {
                        signals = None;
                    }
                }
            },
            None => {
                tick.tick().await;
            }
        }
        let Some(claims) = claims.upgrade() else {
            return;
        };
        // Bounded by the time to live, as a renewal is: a statement stuck on
        // a dead connection must not stop the looks that follow.
        let _ = tokio::time::timeout(claims.ttl, claims.deliver_steering()).await;
    }
}
