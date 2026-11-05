// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The gate itself: who holds a slot, who waits, and who gets the next one
//! (ADR 0045).
//!
//! All of it is under one `std` mutex, never held across an `.await`: a
//! grant, a release and the hand-off of freed slots to waiters are each a
//! few map operations. A waiter is told it was admitted through a oneshot,
//! sent under the lock after the slot has been counted as its own, so a slot
//! is never in two places. A waiter that stops waiting (timed out, or its
//! request dropped) takes its entry out under the lock; if the entry is gone
//! already, the slot was granted to it, and it gives the slot back.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::oneshot;

use super::{Admission, AdmissionLoad};

/// Why the gate did not admit a send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No slot, and no room in the queue (or no queue).
    Full,
    /// Waited `max_wait` with no slot.
    Waited(Duration),
    /// The send's budget ended while it waited, or had nothing left to wait
    /// with.
    Budget,
}

/// The shared gate. Cheap to clone.
#[derive(Clone)]
pub struct AdmissionGate {
    inner: Arc<Inner>,
}

struct Inner {
    config: Admission,
    /// The capacity no tenant has reserved.
    shared_capacity: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Slots held, per tenant; a tenant holding none has no entry.
    held: HashMap<String, usize>,
    /// Slots held beyond their holder's reservation.
    shared_held: usize,
    /// Every slot held.
    running: usize,
    /// Every send waiting.
    queued: usize,
    /// Waiting sends, by priority (served highest first).
    classes: BTreeMap<u8, Class>,
    /// The id the next waiter gets.
    next_id: u64,
}

/// The tenants waiting in one priority, served round robin.
#[derive(Default)]
struct Class {
    /// Tenants with a send waiting, in the order they are served.
    rotation: VecDeque<String>,
    /// Each one's waiting sends, oldest first, and what is left of its turn
    /// in this round.
    waiting: HashMap<String, Waiting>,
}

struct Waiting {
    sends: VecDeque<(u64, oneshot::Sender<()>)>,
    credit: u32,
}

/// A slot of the gate, given back when dropped.
pub struct AdmissionPermit {
    gate: Arc<Inner>,
    tenant: String,
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        let mut state = self.gate.lock();
        self.gate.release(&mut state, &self.tenant);
        self.gate.dispatch(&mut state);
    }
}

impl std::fmt::Debug for AdmissionPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmissionPermit")
            .field("tenant", &self.tenant)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for AdmissionGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmissionGate")
            .field("config", &self.inner.config)
            .field("load", &self.load())
            .finish()
    }
}

/// Takes the waiter's entry out when it stops waiting without a slot, or
/// gives back the slot it was granted as it stopped.
struct WaitGuard<'a> {
    gate: &'a Arc<Inner>,
    tenant: &'a str,
    priority: u8,
    id: u64,
    armed: bool,
}

impl Drop for WaitGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut state = self.gate.lock();
        if !Inner::withdraw(&mut state, self.priority, self.tenant, self.id) {
            // Granted as it stopped waiting: the slot is its, so give it back.
            self.gate.release(&mut state, self.tenant);
            self.gate.dispatch(&mut state);
        }
    }
}

impl AdmissionGate {
    /// A gate for `config`, which [`Admission::invalid`] has passed.
    pub(crate) fn new(config: Admission) -> Self {
        let shared_capacity = config.capacity.saturating_sub(config.reserved());
        Self {
            inner: Arc::new(Inner {
                config,
                shared_capacity,
                state: Mutex::new(State::default()),
            }),
        }
    }

    /// What the gate holds now.
    pub(crate) fn load(&self) -> AdmissionLoad {
        let state = self.inner.lock();
        AdmissionLoad {
            running: state.running,
            queued: state.queued,
            capacity: self.inner.config.capacity,
        }
    }

    /// A slot for `tenant`, now or after a wait of at most the gate's
    /// `max_wait`, or `budget_left` when that is shorter.
    pub(crate) async fn acquire(
        &self,
        tenant: &str,
        budget_left: Option<Duration>,
    ) -> Result<AdmissionPermit, Refusal> {
        let config = &self.inner.config;
        let share = config.share(tenant);
        let (id, admitted) = {
            let mut state = self.inner.lock();
            if self.inner.may_take(&state, tenant) {
                self.inner.take(&mut state, tenant);
                return Ok(self.permit(tenant));
            }
            if state.queued >= config.max_queued || config.max_wait.is_zero() {
                return Err(Refusal::Full);
            }
            if budget_left == Some(Duration::ZERO) {
                return Err(Refusal::Budget);
            }
            let id = state.next_id;
            state.next_id += 1;
            let (tx, rx) = oneshot::channel();
            let class = state.classes.entry(share.priority).or_default();
            let waiting = class.waiting.entry(tenant.to_owned()).or_insert_with(|| {
                class.rotation.push_back(tenant.to_owned());
                Waiting {
                    sends: VecDeque::new(),
                    credit: share.weight,
                }
            });
            waiting.sends.push_back((id, tx));
            state.queued += 1;
            drop(state);
            (id, rx)
        };
        let mut guard = WaitGuard {
            gate: &self.inner,
            tenant,
            priority: share.priority,
            id,
            armed: true,
        };
        let by_budget = budget_left.is_some_and(|left| left < config.max_wait);
        let bound = budget_left.map_or(config.max_wait, |left| left.min(config.max_wait));
        match tokio::time::timeout(bound, admitted).await {
            Ok(Ok(())) => {
                guard.armed = false;
                Ok(self.permit(tenant))
            }
            // The sender is dropped only with the gate; `Waited` is as true
            // an answer as any then.
            _ if by_budget => Err(Refusal::Budget),
            _ => Err(Refusal::Waited(bound)),
        }
    }

    fn permit(&self, tenant: &str) -> AdmissionPermit {
        AdmissionPermit {
            gate: Arc::clone(&self.inner),
            tenant: tenant.to_owned(),
        }
    }
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `tenant` may take a slot: it is below its reservation, or
    /// the shared part has room.
    fn may_take(&self, state: &State, tenant: &str) -> bool {
        let held = state.held.get(tenant).copied().unwrap_or(0);
        held < self.config.share(tenant).reserved || state.shared_held < self.shared_capacity
    }

    /// Counts a slot as `tenant`'s; [`may_take`](Self::may_take) said yes.
    fn take(&self, state: &mut State, tenant: &str) {
        let held = state.held.entry(tenant.to_owned()).or_insert(0);
        if *held >= self.config.share(tenant).reserved {
            state.shared_held += 1;
        }
        *held += 1;
        state.running += 1;
    }

    /// Gives back one of `tenant`'s slots.
    fn release(&self, state: &mut State, tenant: &str) {
        let Some(held) = state.held.get_mut(tenant) else {
            return;
        };
        *held -= 1;
        if *held >= self.config.share(tenant).reserved {
            state.shared_held -= 1;
        }
        if *held == 0 {
            state.held.remove(tenant);
        }
        state.running -= 1;
    }

    /// Takes waiter `id` of `tenant` out of the queue; whether it was still
    /// there.
    fn withdraw(state: &mut State, priority: u8, tenant: &str, id: u64) -> bool {
        let Some(class) = state.classes.get_mut(&priority) else {
            return false;
        };
        let Some(waiting) = class.waiting.get_mut(tenant) else {
            return false;
        };
        let Some(at) = waiting.sends.iter().position(|(w, _)| *w == id) else {
            return false;
        };
        waiting.sends.remove(at);
        if waiting.sends.is_empty() {
            Self::forget(class, tenant);
            if class.waiting.is_empty() {
                state.classes.remove(&priority);
            }
        }
        state.queued -= 1;
        true
    }

    /// Removes `tenant` from `class`, which it has no send waiting in.
    fn forget(class: &mut Class, tenant: &str) {
        class.waiting.remove(tenant);
        class.rotation.retain(|t| t != tenant);
    }

    /// Hands free slots to waiters that may take them, until none can:
    /// the highest priority first, round robin by weight within one.
    fn dispatch(&self, state: &mut State) {
        while let Some((priority, tenant)) = self.next_waiter(state) {
            let Some(class) = state.classes.get_mut(&priority) else {
                return;
            };
            let Some(waiting) = class.waiting.get_mut(&tenant) else {
                return;
            };
            let Some((_, tx)) = waiting.sends.pop_front() else {
                return;
            };
            waiting.credit -= 1;
            let spent = waiting.credit == 0;
            let empty = waiting.sends.is_empty();
            if empty {
                Self::forget(class, &tenant);
            } else if spent {
                waiting.credit = self.config.share(&tenant).weight;
                class.rotation.retain(|t| *t != tenant);
                class.rotation.push_back(tenant.clone());
            }
            if class.waiting.is_empty() {
                state.classes.remove(&priority);
            }
            state.queued -= 1;
            self.take(state, &tenant);
            // A send that fails is a waiter that has stopped waiting and
            // whose guard has not yet taken the lock: the guard will find no
            // entry, read that as a grant, and give the slot back. Releasing
            // it here too would give it back twice.
            let _ = tx.send(());
        }
    }

    /// The waiter the next free slot goes to: in the highest priority with
    /// a tenant that may take one, the first such tenant in its rotation.
    fn next_waiter(&self, state: &State) -> Option<(u8, String)> {
        state.classes.iter().rev().find_map(|(priority, class)| {
            class
                .rotation
                .iter()
                .find(|tenant| self.may_take(state, tenant))
                .map(|tenant| (*priority, tenant.clone()))
        })
    }
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;
