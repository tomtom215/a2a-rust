// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An opt-in admission gate for sends: a capacity, slots reserved per
//! tenant, a bounded wait, priority classes and weighted fairness, all set by
//! the server (audit H9, ADR 0045).
//!
//! Without one, which is the default, a send the server has no room for is
//! refused at once with `Overloaded` (503 with `Retry-After`). With one:
//!
//! * at most [`Admission::capacity`] turns run at once through the gate;
//! * a tenant given [`TenantShare::with_reserved`] slots can always take
//!   them, and no other tenant can;
//! * a send that finds no slot waits, if the queue has room
//!   ([`Admission::with_max_queued`]), for at most
//!   [`Admission::with_max_wait`] or what remains of its budget, whichever is
//!   shorter;
//! * a freed slot goes to the highest [priority](TenantShare::with_priority)
//!   with a send waiting, and within a priority round the tenants waiting,
//!   [`weight`](TenantShare::with_weight) sends each per round.
//!
//! The tenant is the one the handler resolved for the request
//! ([`TenantResolver`](crate::TenantResolver)); nothing the caller sends sets
//! a priority.
//!
//! ```rust
//! use std::time::Duration;
//! use a2a_protocol_server::admission::{Admission, TenantShare};
//!
//! let admission = Admission::new(256)
//!     .with_max_queued(1024)
//!     .with_max_wait(Duration::from_millis(500))
//!     .with_tenant(
//!         "premium",
//!         TenantShare::new().with_priority(1).with_weight(4).with_reserved(32),
//!     );
//! assert_eq!(admission.capacity(), 256);
//! assert_eq!(admission.share("premium").reserved(), 32);
//! assert_eq!(admission.share("anyone").weight(), 1);
//! ```

use std::collections::HashMap;
use std::time::Duration;

pub(crate) mod gate;

/// What one tenant gets from an [`Admission`] gate.
///
/// A tenant with no share of its own gets the gate's
/// [default share](Admission::with_default_share), which reserves nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantShare {
    weight: u32,
    priority: u8,
    reserved: usize,
}

impl TenantShare {
    /// Weight 1, priority 0, nothing reserved.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            weight: 1,
            priority: 0,
            reserved: 0,
        }
    }

    /// How many sends of this tenant are admitted per round, among tenants
    /// of the same priority that are waiting. `0` is taken as `1`.
    #[must_use]
    pub const fn with_weight(mut self, weight: u32) -> Self {
        self.weight = if weight == 0 { 1 } else { weight };
        self
    }

    /// The tenant's class: a freed slot goes to the highest class with a
    /// send waiting that may take it. Higher is served first.
    #[must_use]
    pub const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Slots of the gate's capacity only this tenant may use.
    #[must_use]
    pub const fn with_reserved(mut self, reserved: usize) -> Self {
        self.reserved = reserved;
        self
    }

    /// The weight.
    #[must_use]
    pub const fn weight(&self) -> u32 {
        self.weight
    }

    /// The priority.
    #[must_use]
    pub const fn priority(&self) -> u8 {
        self.priority
    }

    /// The slots reserved.
    #[must_use]
    pub const fn reserved(&self) -> usize {
        self.reserved
    }
}

impl Default for TenantShare {
    /// [`TenantShare::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// The admission gate's configuration; see the [module docs](self). Set it
/// with [`RequestHandlerBuilder::with_admission`](crate::RequestHandlerBuilder::with_admission).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    capacity: usize,
    max_queued: usize,
    max_wait: Duration,
    default_share: TenantShare,
    shares: HashMap<String, TenantShare>,
}

impl Admission {
    /// A gate admitting `capacity` turns at once, refusing the rest (no
    /// queue until [`with_max_queued`](Self::with_max_queued) and
    /// [`with_max_wait`](Self::with_max_wait) are set).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            max_queued: 0,
            max_wait: Duration::ZERO,
            default_share: TenantShare::new(),
            shares: HashMap::new(),
        }
    }

    /// How many sends may wait at once, across tenants. `0`, the default,
    /// waits for nothing.
    #[must_use]
    pub const fn with_max_queued(mut self, max_queued: usize) -> Self {
        self.max_queued = max_queued;
        self
    }

    /// How long a send may wait for a slot. Zero, the default, waits for
    /// nothing. A send whose budget ends sooner waits only that long.
    #[must_use]
    pub const fn with_max_wait(mut self, max_wait: Duration) -> Self {
        self.max_wait = max_wait;
        self
    }

    /// What a tenant with no share of its own gets. It must reserve nothing:
    /// every tenant the server has not named would otherwise get a
    /// reservation of its own.
    #[must_use]
    pub const fn with_default_share(mut self, share: TenantShare) -> Self {
        self.default_share = share;
        self
    }

    /// Gives `tenant` its own share.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>, share: TenantShare) -> Self {
        self.shares.insert(tenant.into(), share);
        self
    }

    /// How many turns run at once through the gate.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many sends may wait at once.
    #[must_use]
    pub const fn max_queued(&self) -> usize {
        self.max_queued
    }

    /// How long a send may wait.
    #[must_use]
    pub const fn max_wait(&self) -> Duration {
        self.max_wait
    }

    /// The share `tenant` gets: its own, or the default.
    #[must_use]
    pub fn share(&self, tenant: &str) -> TenantShare {
        self.shares
            .get(tenant)
            .copied()
            .unwrap_or(self.default_share)
    }

    /// The slots reserved across every named tenant.
    fn reserved(&self) -> usize {
        self.shares
            .values()
            .fold(0usize, |sum, share| sum.saturating_add(share.reserved))
    }

    /// Why this gate cannot work beside a queue cap of `queue_cap`, or `None`.
    pub(crate) fn invalid(&self, queue_cap: usize) -> Option<String> {
        if self.capacity == 0 {
            return Some("the admission gate's capacity must be at least 1".to_owned());
        }
        if self.capacity > queue_cap {
            return Some(format!(
                "the admission gate's capacity ({}) is above the concurrent-stream cap ({queue_cap}), \
                 which would refuse what the gate admitted: lower it, or raise \
                 with_max_concurrent_streams",
                self.capacity
            ));
        }
        if self.default_share.reserved != 0 {
            return Some(
                "the admission gate's default share reserves slots: every tenant not named \
                 would get a reservation of its own; reserve slots per tenant with with_tenant"
                    .to_owned(),
            );
        }
        let reserved = self.reserved();
        if reserved > self.capacity {
            return Some(format!(
                "the admission gate's reservations ({reserved}) are more than its capacity ({})",
                self.capacity
            ));
        }
        None
    }
}

/// What an admission gate holds now: see
/// [`RequestHandler::admission_load`](crate::RequestHandler::admission_load).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AdmissionLoad {
    /// Turns running through the gate.
    pub running: usize,
    /// Sends waiting for a slot.
    pub queued: usize,
    /// The gate's capacity.
    pub capacity: usize,
}

#[cfg(test)]
mod tests;
