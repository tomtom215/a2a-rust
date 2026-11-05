// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Per-origin state: calls in flight, ejection (the circuit breaker), an
//! in-flight cap and a retry-budget ledger (ADR 0032 decision 5, ADR 0050
//! decision 6).
//!
//! An origin is a scheme, host and port, and the address an endpoint is
//! pinned to, if any. Its state is held by an [`HttpPool`](crate::HttpPool),
//! shared by every client of the pool, or by one balanced client for itself.
//!
//! # Ejection
//!
//! `consecutive_failures` failures in a row eject the origin; the n-th
//! ejection in a row lasts `base * 2^(n-1)`, capped at `max`. When it ends,
//! one call is let through as a probe: its success closes the origin and its
//! failure ejects it again. A balancer that must keep an ejected origin in
//! rotation sends it *forced* calls: a success closes it, a failure changes
//! nothing (it is already out).

pub mod transport;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

use crate::balance::Ejection;
use crate::error::ClientError;
use crate::retry::RetryBudget;
use crate::retry::budget::Ledger;

pub use transport::OriginTransport;

/// How many origins a registry keeps before it forgets idle, healthy ones.
const PRUNE_ABOVE: usize = 1024;

/// What one call to an origin is, for its ejection state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ticket {
    /// An ordinary call to an origin in rotation.
    Normal,
    /// The one call let through when an ejection has ended.
    Probe,
    /// A call to an ejected origin a balancer kept in rotation.
    Forced,
}

/// What a call's result says about the origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The origin answered (an error answer included).
    Success,
    /// The origin could not be reached or could not answer.
    Failure,
    /// Nothing was learned: the call was refused locally, never sent, or
    /// cancelled.
    Neutral,
}

/// What `result` says about the origin it came from (ADR 0050 decision 3).
pub const fn outcome_of<T>(result: &Result<T, ClientError>) -> Outcome {
    match result {
        Ok(_) => Outcome::Success,
        Err(e) => match e {
            ClientError::Http(_) | ClientError::HttpClient(_) | ClientError::Timeout(_) => {
                Outcome::Failure
            }
            ClientError::UnexpectedStatus {
                status: 502..=504, ..
            } => Outcome::Failure,
            ClientError::TooManyPendingRequests { .. }
            | ClientError::CircuitOpen { .. }
            | ClientError::Config(_)
            | ClientError::InvalidEndpoint(_)
            | ClientError::InsecureTransport(_)
            | ClientError::InsecureWebhook(_)
            | ClientError::CapabilityNotDeclared { .. }
            | ClientError::InvalidAuthScheme(_) => Outcome::Neutral,
            _ => Outcome::Success,
        },
    }
}

/// The ejection state of one origin.
#[derive(Debug, Default)]
struct Health {
    failures: u32,
    ejected_until: Option<Instant>,
    ejections: u32,
    probing: bool,
}

/// What a recorded outcome changed, for the log.
enum Change {
    None,
    Closed,
    Ejected(Duration),
}

/// Where an origin stands for a balancer choosing among several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// In rotation; a call to it is the ticket given.
    Available(Ticket),
    /// Ejected until then (or, with `None`, while its probe is running).
    Ejected(Option<Instant>),
}

/// The state of one origin.
pub struct OriginState {
    key: String,
    ejection: Option<Ejection>,
    health: Mutex<Health>,
    in_flight: AtomicUsize,
    cap: Option<(Arc<Semaphore>, usize)>,
    budget: Option<Ledger>,
}

impl OriginState {
    fn new(
        key: String,
        ejection: Option<Ejection>,
        cap: Option<usize>,
        budget: Option<RetryBudget>,
    ) -> Self {
        Self {
            key,
            ejection,
            health: Mutex::new(Health::default()),
            in_flight: AtomicUsize::new(0),
            cap: cap.map(|n| (Arc::new(Semaphore::new(n)), n)),
            budget: budget.map(Ledger::new),
        }
    }

    fn health(&self) -> std::sync::MutexGuard<'_, Health> {
        self.health.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Calls in flight to this origin (a stream counts until it is dropped).
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Where the origin stands at `now`. Taking a probe slot is a side
    /// effect: `Available(Probe)` claims it, so only one caller gets it.
    pub fn standing(&self, now: Instant) -> Standing {
        // Without a policy nothing is ever ejected, and this is the first arm.
        let mut health = self.health();
        match health.ejected_until {
            None => Standing::Available(Ticket::Normal),
            Some(until) if now < until => Standing::Ejected(Some(until)),
            Some(_) if health.probing => Standing::Ejected(None),
            Some(_) => {
                health.probing = true;
                drop(health);
                Standing::Available(Ticket::Probe)
            }
        }
    }

    /// Whether the origin is out of rotation at `now` (ejected, or ended
    /// and its probe running), without claiming anything.
    pub fn is_ejected(&self, now: Instant) -> bool {
        let health = self.health();
        health
            .ejected_until
            .is_some_and(|until| now < until || health.probing)
    }

    /// When the current ejection ends; `None` when not ejected.
    pub fn ejected_until(&self) -> Option<Instant> {
        self.health().ejected_until
    }

    /// Hands back a probe slot claimed by [`standing`](Self::standing) and
    /// not used.
    pub fn release_probe(&self) {
        self.health().probing = false;
    }

    /// Admits a call to an origin used on its own (not balanced): refused
    /// with [`ClientError::CircuitOpen`] while it is ejected.
    pub fn admit(&self, now: Instant) -> Result<Ticket, ClientError> {
        match self.standing(now) {
            Standing::Available(ticket) => Ok(ticket),
            Standing::Ejected(until) => Err(ClientError::CircuitOpen {
                origin: self.key.clone(),
                retry_after: until.map(|until| until.saturating_duration_since(now)),
            }),
        }
    }

    /// Starts a call under `ticket`: takes a place under the in-flight cap,
    /// if any, and counts the call in flight until the guard is dropped.
    ///
    /// # Errors
    ///
    /// [`ClientError::TooManyPendingRequests`] when the cap is reached; a
    /// probe slot the ticket held is handed back.
    pub fn enter(self: &Arc<Self>, ticket: Ticket) -> Result<CallGuard, ClientError> {
        let permit = match &self.cap {
            Some((sem, limit)) => {
                let Ok(permit) = Arc::clone(sem).try_acquire_owned() else {
                    if ticket == Ticket::Probe {
                        self.release_probe();
                    }
                    return Err(ClientError::TooManyPendingRequests { limit: *limit });
                };
                Some(permit)
            }
            None => None,
        };
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        Ok(CallGuard {
            state: Arc::clone(self),
            ticket,
            recorded: false,
            permit,
        })
    }

    /// Records what a call under `ticket` ended in.
    fn record(&self, ticket: Ticket, outcome: Outcome, now: Instant) {
        let Some(policy) = self.ejection else {
            return;
        };
        let mut health = self.health();
        let change = Self::apply(&mut health, ticket, outcome, policy, now);
        drop(health);
        match change {
            Change::None => {}
            Change::Closed => {
                trace_info!(origin = %self.key, "origin back in rotation");
            }
            Change::Ejected(_lasts) => {
                trace_warn!(origin = %self.key, ?_lasts, "origin ejected after failing in a row");
            }
        }
    }

    /// The state machine: what `outcome` of a call under `ticket` does to
    /// `health`.
    fn apply(
        health: &mut Health,
        ticket: Ticket,
        outcome: Outcome,
        policy: Ejection,
        now: Instant,
    ) -> Change {
        if ticket == Ticket::Probe {
            health.probing = false;
        }
        match (ticket, outcome) {
            (_, Outcome::Neutral) | (Ticket::Forced, Outcome::Failure) => Change::None,
            (Ticket::Normal, Outcome::Success) => {
                health.failures = 0;
                Change::None
            }
            (Ticket::Probe | Ticket::Forced, Outcome::Success) => {
                let was_ejected = health.ejected_until.is_some();
                *health = Health::default();
                if was_ejected {
                    Change::Closed
                } else {
                    Change::None
                }
            }
            (Ticket::Normal, Outcome::Failure) => {
                if health.ejected_until.is_some() {
                    return Change::None;
                }
                health.failures += 1;
                if health.failures < policy.consecutive_failures {
                    return Change::None;
                }
                Change::Ejected(Self::eject(health, policy, now))
            }
            (Ticket::Probe, Outcome::Failure) => Change::Ejected(Self::eject(health, policy, now)),
        }
    }

    /// Ejects the origin for its next ejection's length, which it returns.
    fn eject(health: &mut Health, policy: Ejection, now: Instant) -> Duration {
        health.ejections = health.ejections.saturating_add(1);
        let lasts = ejection_length(policy, health.ejections);
        health.ejected_until = Some(now + lasts);
        health.failures = 0;
        lasts
    }

    /// The pool's retry ledger for this origin, if it has one.
    pub const fn budget(&self) -> Option<&Ledger> {
        self.budget.as_ref()
    }

    /// A snapshot for [`HttpPool::origins`](crate::HttpPool::origins).
    pub fn snapshot(&self, now: Instant) -> OriginSnapshot {
        let health = self.health();
        OriginSnapshot {
            origin: self.key.clone(),
            in_flight: self.in_flight(),
            ejected_for: health
                .ejected_until
                .map(|until| until.saturating_duration_since(now)),
            consecutive_failures: health.failures,
            consecutive_ejections: health.ejections,
        }
    }
}

impl std::fmt::Debug for OriginState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OriginState")
            .field("origin", &self.key)
            .field("in_flight", &self.in_flight())
            .field("health", &*self.health())
            .finish_non_exhaustive()
    }
}

/// How long the `n`-th ejection in a row lasts: `base * 2^(n-1)`, at most
/// `max`.
pub fn ejection_length(policy: Ejection, n: u32) -> Duration {
    let doublings = n.saturating_sub(1).min(31);
    policy
        .base
        .checked_mul(1_u32 << doublings)
        .map_or(policy.max, |d| d.min(policy.max))
}

/// One call to an origin: counted in flight until dropped. A guard dropped
/// without [`record`](Self::record) records nothing learned.
pub struct CallGuard {
    state: Arc<OriginState>,
    ticket: Ticket,
    recorded: bool,
    permit: Option<OwnedSemaphorePermit>,
}

impl CallGuard {
    /// Records what the call ended in. A stream's guard is kept after this,
    /// and counts the stream in flight until it is dropped.
    pub fn record(&mut self, outcome: Outcome) {
        if !self.recorded {
            self.recorded = true;
            self.state.record(self.ticket, outcome, Instant::now());
        }
    }

    /// Gives back the place under the in-flight cap (a stream's, once it has
    /// opened), keeping the call counted in flight.
    pub fn release_permit(&mut self) {
        self.permit = None;
    }
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        self.record(Outcome::Neutral);
        self.state.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl std::fmt::Debug for CallGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallGuard")
            .field("origin", &self.state.key)
            .field("ticket", &self.ticket)
            .field("recorded", &self.recorded)
            .finish_non_exhaustive()
    }
}

/// The state of one origin at one moment, from
/// [`HttpPool::origins`](crate::HttpPool::origins).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct OriginSnapshot {
    /// `scheme://host:port`, and ` via ADDR` for an endpoint pinned to an
    /// address. Never a user name, password, path or query.
    pub origin: String,
    /// Calls in flight (a stream counts until it is dropped).
    pub in_flight: usize,
    /// How much longer the origin stays ejected: `Some(ZERO)` once the
    /// ejection has ended and the probe has not yet closed it; `None` when
    /// it is in rotation.
    pub ejected_for: Option<Duration>,
    /// Failures in a row since the last success or ejection.
    pub consecutive_failures: u32,
    /// Ejections in a row since the last success.
    pub consecutive_ejections: u32,
}

/// The origins of a pool, or of one balanced client.
pub struct Origins {
    ejection: Option<Ejection>,
    cap: Option<usize>,
    budget: Option<RetryBudget>,
    map: Mutex<HashMap<String, Arc<OriginState>>>,
}

impl Origins {
    pub fn new(
        ejection: Option<Ejection>,
        cap: Option<usize>,
        budget: Option<RetryBudget>,
    ) -> Self {
        Self {
            ejection,
            cap,
            budget,
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Whether a client that is not balanced needs its calls to pass
    /// through the origin's state: there is a breaker or a cap to apply.
    pub const fn enforces(&self) -> bool {
        self.ejection.is_some() || self.cap.is_some()
    }

    /// Whether the origins keep a retry ledger.
    pub const fn has_budget(&self) -> bool {
        self.budget.is_some()
    }

    /// The state of the origin named `key`, made on first use.
    pub fn get(&self, key: &str) -> Arc<OriginState> {
        let mut map = self.map.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(state) = map.get(key) {
            return Arc::clone(state);
        }
        if map.len() >= PRUNE_ABOVE {
            // Forget origins nothing holds and nothing has against them.
            map.retain(|_, s| Arc::strong_count(s) > 1 || s.ejected_until().is_some());
        }
        let state = Arc::new(OriginState::new(
            key.to_owned(),
            self.ejection,
            self.cap,
            self.budget,
        ));
        map.insert(key.to_owned(), Arc::clone(&state));
        drop(map);
        state
    }

    /// Every origin's state, sorted by origin.
    pub fn snapshot(&self) -> Vec<OriginSnapshot> {
        let now = Instant::now();
        let map = self.map.lock().unwrap_or_else(PoisonError::into_inner);
        let mut all: Vec<OriginSnapshot> = map.values().map(|s| s.snapshot(now)).collect();
        drop(map);
        all.sort_by(|a, b| a.origin.cmp(&b.origin));
        all
    }
}

impl std::fmt::Debug for Origins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Origins")
            .field("ejection", &self.ejection)
            .field("cap", &self.cap)
            .field("budget", &self.budget)
            .field("origins", &self.snapshot())
            .finish_non_exhaustive()
    }
}

/// The origin of `url`: `scheme://host:port`, the port defaulted by the
/// scheme, and ` via ADDR` when pinned. `None` when `url` has no host.
pub fn origin_key(url: &str, pinned: Option<std::net::SocketAddr>) -> Option<String> {
    let uri: hyper::Uri = url.parse().ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    let host = uri.host()?.to_ascii_lowercase();
    let port = uri
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let origin = format!("{scheme}://{host}:{port}");
    Some(pinned.map_or_else(|| origin.clone(), |addr| format!("{origin} via {addr}")))
}

#[cfg(test)]
mod tests;
