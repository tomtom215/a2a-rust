// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One delivery's place in the gate, shared between the code that bounds it
//! in time (`deliver_once` in the handler's push delivery) and the code that
//! makes it wait (the gate, and the sender once it knows the addresses).
//!
//! # Why the clock starts at admission
//!
//! Until 0.16.0 a delivery's wait for a permit counted against
//! `push_delivery_timeout`, so a gate held full by one caller's tarpits made
//! every other caller's webhooks end as `timeout` without a request being
//! made (audit NEW-f-2). Now the wait has its own bound, the same length, and
//! the time a webhook is given starts when its delivery is let through: a
//! delivery that is never let through ends as
//! [`push_outcome::THROTTLED`](crate::metrics::push_outcome::THROTTLED), one
//! that is let through has the whole of `push_delivery_timeout` for its
//! webhook.
//!
//! # Why the address bound can give permits back
//!
//! The destination bound is taken by name before any lookup, and by address
//! once the sender has resolved the name (audit NEW-d-3: a thousand names in
//! a wildcard record are one victim). The address is known only after the
//! delivery holds its partition and overall permits, and a delivery that
//! waited for an address while holding them would let one victim's queue
//! hold every overall permit, the starvation the name bound was ordered to
//! prevent. So a delivery whose address is full gives its partition and
//! overall permits back, waits for the address, and takes them again.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::OwnedSemaphorePermit;
use tokio::time::Instant;

use super::{GateShared, Slot};

tokio::task_local! {
    /// The admission of the delivery being polled, set by `deliver_once`.
    static ADMISSION: Arc<Admission>;
}

/// The permits a delivery holds in the gate past its name slot.
#[derive(Default)]
pub(super) struct Held {
    pub(super) partition: Option<Slot>,
    pub(super) overall: Option<OwnedSemaphorePermit>,
    /// The delivery's wait has been reported (once per delivery).
    pub(super) reported: bool,
}

#[derive(Default)]
struct State {
    /// When the delivery's time is up, if it has a clock.
    deadline: Option<Instant>,
    /// Waiting for a permit now.
    waiting: bool,
    /// The gate the delivery is in and what it holds there, while it is in.
    gate: Option<(Arc<GateShared>, Arc<Mutex<Held>>)>,
}

/// One delivery's admission; see the module docs.
pub struct Admission {
    timeout: Duration,
    state: Mutex<State>,
}

impl std::fmt::Debug for Admission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.lock();
        f.debug_struct("Admission")
            .field("timeout", &self.timeout)
            .field("waiting", &state.waiting)
            .finish_non_exhaustive()
    }
}

impl Admission {
    /// A delivery that has `timeout` from now, and `timeout` again from each
    /// time the gate lets it through.
    pub fn new(timeout: Duration) -> Arc<Self> {
        Arc::new(Self {
            timeout,
            state: Mutex::new(State {
                deadline: Instant::now().checked_add(timeout),
                ..State::default()
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `f` as this delivery.
    pub async fn scope<F: std::future::Future>(self: &Arc<Self>, f: F) -> F::Output {
        ADMISSION.scope(Arc::clone(self), f).await
    }

    /// When the delivery's time is up: `None` for a timeout too long to
    /// represent.
    pub fn deadline(&self) -> Option<Instant> {
        self.lock().deadline
    }

    /// Whether the delivery is waiting for a permit: one cut off now was
    /// never sent, and says nothing about its webhook.
    pub fn waiting(&self) -> bool {
        self.lock().waiting
    }

    pub(super) fn begin_wait(&self) {
        self.lock().waiting = true;
    }

    /// Let through: the webhook's time starts now.
    pub(super) fn admit(&self) {
        let mut state = self.lock();
        state.waiting = false;
        state.deadline = Instant::now().checked_add(self.timeout);
    }

    pub(super) fn enter(&self, gate: Arc<GateShared>, held: Arc<Mutex<Held>>) {
        self.lock().gate = Some((gate, held));
    }

    pub(super) fn leave(&self) {
        self.lock().gate = None;
    }

    fn gate(&self) -> Option<(Arc<GateShared>, Arc<Mutex<Held>>)> {
        self.lock().gate.clone()
    }
}

/// The admission of the delivery being polled, if it has one.
pub fn current() -> Option<Arc<Admission>> {
    ADMISSION.try_with(Arc::clone).ok()
}

/// What a delivery holds on the addresses it connects to; released on drop.
#[must_use = "the address bound ends when this is dropped"]
pub struct AddressHold {
    /// Held for their drop, which gives the permits back.
    slots: Vec<Slot>,
}

impl std::fmt::Debug for AddressHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddressHold")
            .field("addresses", &self.slots.len())
            .finish()
    }
}

/// Counts the delivery being polled against each of `addrs`, the addresses
/// its webhook's name was checked to resolve to, under the gate's
/// per-destination bound; see the module docs. `None` when the delivery is
/// not in a gate, or there are no addresses.
///
/// Addresses are taken in a fixed order (sorted, without repeats) so two
/// deliveries to overlapping sets cannot each hold one and wait for the
/// other's.
pub async fn hold_addresses(addrs: &[std::net::IpAddr]) -> Option<AddressHold> {
    let admission = current()?;
    let (gate, held) = admission.gate()?;
    let mut keys: Vec<String> = addrs.iter().map(|ip| format!("ip:{ip}")).collect();
    keys.sort_unstable();
    keys.dedup();
    if keys.is_empty() {
        return None;
    }
    let mut slots: Vec<Slot> = keys
        .into_iter()
        .map(|key| gate.addresses.slot(key))
        .collect();
    let free = slots.iter_mut().all(Slot::try_take);
    if !free {
        // Give back what the gate let this delivery hold, so it is not held
        // while it waits; take it again once the addresses are free.
        let (partition_sem, had_overall) = {
            let mut held = held.lock().unwrap_or_else(PoisonError::into_inner);
            let partition = held.partition.as_mut().map(|slot| {
                drop(slot.permit.take());
                Arc::clone(&slot.sem)
            });
            gate.report_wait(&mut held.reported);
            (partition, held.overall.take().is_some())
        };
        admission.begin_wait();
        for slot in &mut slots {
            slot.take().await;
        }
        if let Some(sem) = partition_sem {
            let permit = sem.acquire_owned().await.ok();
            if let Some(slot) = held
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .partition
                .as_mut()
            {
                slot.permit = permit;
            }
        }
        if had_overall {
            let permit = Arc::clone(&gate.overall).acquire_owned().await.ok();
            held.lock().unwrap_or_else(PoisonError::into_inner).overall = permit;
        }
        admission.admit();
    }
    Some(AddressHold { slots })
}
