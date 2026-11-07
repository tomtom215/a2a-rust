// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ByteBudget`]: one process-wide bound on the bytes of events held in
//! event queues (audit H13, ADR 0047).
//!
//! Each event a writer accepts is charged its serialized size, and the
//! [`Charge`] rides inside the event, which both the persistence channel and
//! the broadcast channel hold by the same `Arc`. It is returned when the last
//! holder drops the event: the processor once it is stored, each subscriber
//! once it is sent, the broadcast slot once every subscriber has read it. So
//! what the budget counts is what is held, including what a subscriber that
//! stopped reading still pins.

use std::sync::Arc;

use tokio::sync::Semaphore;

/// The shared budget. Cheap to clone.
#[derive(Clone)]
pub struct ByteBudget {
    bytes: Arc<Semaphore>,
    total: usize,
}

/// Bytes charged to a [`ByteBudget`], returned when the last clone drops.
#[derive(Clone)]
pub struct Charge(Arc<Held>);

/// The bytes one charge holds. Not a tokio permit: a permit is a guard to
/// clippy, and an event carrying one made every type that carries an event
/// read as a lock held across the send path. The permit is forgotten when
/// taken and its bytes are added back here.
struct Held {
    bytes: Arc<Semaphore>,
    n: u32,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.bytes.add_permits(self.n as usize);
    }
}

impl Charge {
    /// Takes over `n` bytes whose permit was just forgotten.
    fn of(bytes: &Arc<Semaphore>, n: u32) -> Self {
        Self(Arc::new(Held {
            bytes: Arc::clone(bytes),
            n,
        }))
    }
}

impl std::fmt::Debug for Charge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Charge({})", self.0.n)
    }
}

impl std::fmt::Debug for ByteBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ByteBudget")
            .field("total", &self.total)
            .field("held", &self.held())
            .finish_non_exhaustive()
    }
}

impl ByteBudget {
    /// A budget of `total` bytes, at least one.
    pub(crate) fn new(total: usize) -> Self {
        let total = total.clamp(1, Semaphore::MAX_PERMITS);
        Self {
            bytes: Arc::new(Semaphore::new(total)),
            total,
        }
    }

    /// The budget's size.
    pub(crate) const fn total(&self) -> usize {
        self.total
    }

    /// Bytes charged and not yet returned.
    pub(crate) fn held(&self) -> usize {
        self.total - self.bytes.available_permits()
    }

    /// What one event of `size` bytes is charged: its size, but never more
    /// than the whole budget (or a `u32`), so that one large event can
    /// always be accepted once the budget is otherwise empty.
    fn cost(&self, size: usize) -> u32 {
        u32::try_from(size.min(self.total)).unwrap_or(u32::MAX)
    }

    /// Charges `size` bytes now, or `None` when the budget has no room.
    pub(crate) fn try_charge(&self, size: usize) -> Option<Charge> {
        let n = self.cost(size);
        self.bytes.try_acquire_many(n).ok()?.forget();
        Some(Charge::of(&self.bytes, n))
    }

    /// Charges `size` bytes once there is room, or `None` at `deadline`.
    pub(crate) async fn charge_by(
        &self,
        size: usize,
        deadline: tokio::time::Instant,
    ) -> Option<Charge> {
        let n = self.cost(size);
        match tokio::time::timeout_at(deadline, self.bytes.acquire_many(n)).await {
            Ok(Ok(permit)) => {
                permit.forget();
                Some(Charge::of(&self.bytes, n))
            }
            _ => None,
        }
    }

    /// Whether a new turn may start: at least a tenth of the budget is free,
    /// kept for the turns already running.
    pub(crate) fn admits_new_work(&self) -> bool {
        self.bytes.available_permits() >= (self.total / 10).max(1)
    }
}

#[cfg(test)]
#[path = "byte_budget_tests.rs"]
mod tests;
