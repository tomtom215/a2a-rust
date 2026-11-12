// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A durable queue of webhook deliveries (ADR 0037, audit H14).
//!
//! Without one, push delivery is a few tries held in memory: a delivery
//! waiting in a task's lane, or being retried, when the process stops is not
//! delivered by this replica or any other, terminal events included, and
//! nothing records that it was lost. With
//! [`RequestHandlerBuilder::with_push_outbox`](crate::RequestHandlerBuilder::with_push_outbox),
//! each event's delivery to each of its task's webhooks is a row: written by
//! the task's lane once the event is stored, claimed by a worker on whichever
//! replica gets to it first, retried with backoff until the webhook answers
//! or the row is older than
//! [`push_outbox_max_age`](crate::handler::HandlerLimits::push_outbox_max_age),
//! and removed when it is done.
//!
//! # What a receiver sees
//!
//! At least once: a delivery whose answer was lost, or whose replica stopped
//! after sending it, is sent again, from any replica, with the same
//! `A2A-Delivery-Id` ([`webhook::DELIVERY_ID_HEADER`](super::webhook::DELIVERY_ID_HEADER)),
//! which is the row's. A webhook sees a task's events in the order they were
//! stored: a row is claimed only when no earlier row of the same webhook and
//! task is unfinished.
//!
//! # Stores
//!
//! [`InMemoryPushOutbox`] keeps nothing across a restart, and is the same
//! code path for a deployment whose other stores are in memory;
//! `SqlitePushOutbox` and `PostgresPushOutbox` (features `sqlite`,
//! `postgres`) keep the rows in a table of their own, `a2a_push_outbox`, and
//! the PostgreSQL one is shared by every replica that opens it.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;

mod memory;
pub use memory::InMemoryPushOutbox;

#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod body;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;
#[cfg(feature = "postgres")]
pub use postgres::PostgresPushOutbox;
#[cfg(feature = "sqlite")]
pub use sqlite::SqlitePushOutbox;

/// A boxed future, as every [`PushOutbox`] method returns.
pub type OutboxFuture<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

/// One event's delivery to one webhook, to be recorded.
#[derive(Debug, Clone)]
pub struct NewDelivery {
    /// Sent as `A2A-Delivery-Id` on every attempt.
    pub delivery_id: String,
    /// The tenant the delivery runs as.
    pub tenant: String,
    /// The caller the delivery runs as, where callers are isolated.
    pub caller: Option<String>,
    /// The task the event belongs to.
    pub task_id: String,
    /// The push config's id; the config itself is read when the delivery is
    /// made, so its credentials stay in the config store.
    pub config_id: String,
    /// The event.
    pub event: StreamResponse,
    /// It carries a terminal or interrupted status: never dropped to make
    /// room.
    pub ends_turn: bool,
}

/// A delivery a worker has claimed.
#[derive(Debug, Clone)]
pub struct ClaimedDelivery {
    /// The row's position: what [`PushOutbox::complete`] and
    /// [`PushOutbox::retry_after`] name it by. Increasing in the order rows
    /// were recorded.
    pub seq: i64,
    /// What was recorded.
    pub delivery: NewDelivery,
    /// Attempts made before this claim.
    pub attempts: u32,
    /// How long ago it was recorded, by the store's clock.
    pub age: Duration,
}

/// A reservation whose hold has run out.
///
/// The deliveries of one event were reserved before the event was stored and
/// never armed or released. The replica that reserved them stopped, or is
/// stalled, so something else has to decide whether the event was stored (see
/// [`PushOutbox::reserve`]).
#[derive(Debug, Clone)]
pub struct Held {
    /// The reservation's id: what [`PushOutbox::arm`] and
    /// [`PushOutbox::release`] name it by.
    pub reservation: String,
    /// The tenant the deliveries run as.
    pub tenant: String,
    /// The caller they run as, where callers are isolated.
    pub caller: Option<String>,
    /// The task the event belongs to.
    pub task_id: String,
    /// The event's position in its task's log.
    pub log_seq: u64,
    /// The event.
    pub event: StreamResponse,
}

/// Where webhook deliveries wait until they are delivered; see the module
/// documentation.
///
/// A row is claimed by one owner at a time, under a lease: until the lease
/// runs out no other claim returns it, and once it has, any may. An owner
/// that stops mid-delivery leaves its rows to the next claim after their
/// leases.
pub trait PushOutbox: Send + Sync + 'static {
    /// Records `deliveries`, in order. A delivery whose `delivery_id` is already
    /// in the outbox is left as it is (recording the same delivery twice keeps
    /// one row, so a caller can record again what it is unsure was recorded);
    /// one that was delivered and removed is recorded again, and delivered again
    /// under the same id, which a receiver sees as a retry. At most `per_webhook` rows wait for one
    /// webhook of one task: past that, recording one drops the oldest of them
    /// that is not claimed and does not end a turn, or, when there is none,
    /// the new one unless it ends a turn (kept past the bound). Returns how
    /// many were dropped.
    fn record(&self, deliveries: Vec<NewDelivery>, per_webhook: usize) -> OutboxFuture<'_, usize>;

    /// Claims up to `max` rows that are due, not under another owner's lease,
    /// and have no earlier unfinished row of the same tenant, caller, task and
    /// config, for `owner`, for `lease`. In record order.
    fn claim<'a>(
        &'a self,
        owner: &'a str,
        max: usize,
        lease: Duration,
    ) -> OutboxFuture<'a, Vec<ClaimedDelivery>>;

    /// Removes row `seq`: delivered, refused for good, expired, or its config
    /// is gone. Harmless when it is already gone.
    fn complete(&self, seq: i64) -> OutboxFuture<'_, ()>;

    /// Makes row `seq` due again `after` from now, one attempt more, and
    /// releases it, when `owner` still holds its lease; otherwise does
    /// nothing (another owner has it, and will finish it).
    fn retry_after<'a>(&'a self, seq: i64, owner: &'a str, after: Duration)
    -> OutboxFuture<'a, ()>;

    /// How many rows wait, claimed or not, reserved or not: for a gauge.
    fn pending(&self) -> OutboxFuture<'_, usize>;

    /// Whether this outbox renews leases ([`renew`](Self::renew)). With renewal a
    /// worker claims for a short lease and renews it while it works, so the
    /// rows of a worker that died are free after one short lease, not after the
    /// longest a delivery can take; without it (the default) the worker takes
    /// the long lease.
    fn supports_renewal(&self) -> bool {
        false
    }

    /// Extends to `lease` from now every unexpired lease `owner` holds: the
    /// worker's heartbeat. Returns how many rows it renewed.
    ///
    /// # Errors
    ///
    /// The outbox cannot be written, or does not renew.
    fn renew<'a>(&'a self, owner: &'a str, lease: Duration) -> OutboxFuture<'a, usize> {
        let _ = (owner, lease);
        Box::pin(async {
            Err(a2a_protocol_types::error::A2aError::unsupported_operation(
                "this push outbox does not renew leases",
            ))
        })
    }

    /// Whether this outbox keeps reservations ([`reserve`](Self::reserve)).
    /// One that does not (the default) is recorded in only after an event is
    /// stored, and a crash between the two loses that event's deliveries.
    fn supports_reservations(&self) -> bool {
        false
    }

    /// Phase one of recording an event's deliveries *before* the event is
    /// stored (audit H14): `deliveries`, all of one event at position
    /// `log_seq` of its task's log, are written **unarmed**.
    ///
    /// An unarmed row is not claimed, but it holds its webhook's place: no
    /// later row of the same webhook is claimed past it, so the order of a
    /// webhook's deliveries is the order of the events whether or not they
    /// have been stored yet. It is never dropped to make room for another
    /// row. The caller then either [`arm`](Self::arm)s the reservation once
    /// the event is stored, or [`release`](Self::release)s it if it was not.
    ///
    /// If the caller stops between the two, the rows are left with a `hold`
    /// that runs out; [`held`](Self::held) then lists the reservation, and
    /// whoever settles it decides from the task store whether the event was
    /// stored. Like [`record`](Self::record), returns how many rows were
    /// dropped for the `per_webhook` bound.
    ///
    /// # Errors
    ///
    /// The outbox cannot be written, or keeps no reservations.
    fn reserve<'a>(
        &'a self,
        reservation: &'a str,
        log_seq: u64,
        deliveries: Vec<NewDelivery>,
        per_webhook: usize,
        hold: Duration,
    ) -> OutboxFuture<'a, usize> {
        let _ = (reservation, log_seq, deliveries, per_webhook, hold);
        Box::pin(async {
            Err(a2a_protocol_types::error::A2aError::unsupported_operation(
                "this push outbox keeps no reservations",
            ))
        })
    }

    /// Phase two, the event was stored: the rows of `reservation` become
    /// claimable. Returns how many rows the reservation has, armed by this
    /// call or already; `0` means there is nothing of it left (it was
    /// released, or settled by another replica as not stored, or already
    /// delivered), and the caller records the deliveries afresh.
    ///
    /// # Errors
    ///
    /// The outbox cannot be written, or keeps no reservations.
    fn arm<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, usize> {
        let _ = reservation;
        Box::pin(async {
            Err(a2a_protocol_types::error::A2aError::unsupported_operation(
                "this push outbox keeps no reservations",
            ))
        })
    }

    /// The event was not stored: removes the rows of `reservation` that are
    /// not armed. Armed rows, and a reservation that is gone, are left
    /// alone.
    ///
    /// # Errors
    ///
    /// The outbox cannot be written, or keeps no reservations.
    fn release<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, ()> {
        let _ = reservation;
        Box::pin(async {
            Err(a2a_protocol_types::error::A2aError::unsupported_operation(
                "this push outbox keeps no reservations",
            ))
        })
    }

    /// Every reservation of one task that is not armed, whether or not its hold
    /// has run out: for a caller that knows the reserver can write no more, such
    /// as the reaper once it holds the task's turn claim (a store refuses a
    /// writer whose claim was taken over, so what the task store holds is final).
    ///
    /// # Errors
    ///
    /// The outbox cannot be read, or keeps no reservations.
    fn held_for_task<'a>(
        &'a self,
        tenant: &'a str,
        caller: Option<&'a str>,
        task_id: &'a str,
    ) -> OutboxFuture<'a, Vec<Held>> {
        let _ = (tenant, caller, task_id);
        Box::pin(async {
            Err(a2a_protocol_types::error::A2aError::unsupported_operation(
                "this push outbox keeps no reservations",
            ))
        })
    }

    /// Up to `limit` reservations that are not armed and whose hold has run
    /// out, oldest first, one entry each however many webhooks it has rows
    /// for.
    ///
    /// # Errors
    ///
    /// The outbox cannot be read, or keeps no reservations.
    fn held(&self, limit: usize) -> OutboxFuture<'_, Vec<Held>> {
        let _ = limit;
        Box::pin(async {
            Err(a2a_protocol_types::error::A2aError::unsupported_operation(
                "this push outbox keeps no reservations",
            ))
        })
    }
}

/// The denominator of a jitter: [`jitter`] is a fraction of it.
pub const JITTER_ONE: u32 = 65_536;

/// How long a delivery that failed `attempts` times waits before the next.
///
/// 1 s doubled per attempt, at most `max`, less up to a fifth of jitter
/// (`jitter` out of [`JITTER_ONE`]), so retries of many rows that failed
/// together do not arrive together. In whole nanoseconds, so it is exact.
#[must_use]
pub fn backoff(attempts: u32, max: Duration, jitter: u32) -> Duration {
    let full = Duration::from_secs(1)
        .checked_mul(
            1u32.checked_shl(attempts.saturating_sub(1))
                .unwrap_or(u32::MAX),
        )
        .unwrap_or(max)
        .min(max);
    let fifths = JITTER_ONE * 5;
    full * (fifths - jitter.min(JITTER_ONE)) / fifths
}

/// A jitter out of [`JITTER_ONE`], from a fresh random UUID: no new
/// dependency for a number that needs to be spread, not unpredictable.
#[must_use]
pub fn jitter() -> u32 {
    let [a, b, ..] = *uuid::Uuid::new_v4().as_bytes();
    u32::from(u16::from_be_bytes([a, b]))
}

#[cfg(test)]
mod contract;
#[cfg(test)]
mod postgres_tests;
#[cfg(test)]
mod sqlite_tests;
#[cfg(test)]
mod tests;
