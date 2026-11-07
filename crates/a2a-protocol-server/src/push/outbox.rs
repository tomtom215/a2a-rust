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

/// Where webhook deliveries wait until they are delivered; see the module
/// documentation.
///
/// A row is claimed by one owner at a time, under a lease: until the lease
/// runs out no other claim returns it, and once it has, any may. An owner
/// that stops mid-delivery leaves its rows to the next claim after their
/// leases.
pub trait PushOutbox: Send + Sync + 'static {
    /// Records `deliveries`, in order. At most `per_webhook` rows wait for one
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

    /// How many rows wait, claimed or not: for a gauge.
    fn pending(&self) -> OutboxFuture<'_, usize>;
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
