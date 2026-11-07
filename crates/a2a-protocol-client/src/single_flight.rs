// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One run of an operation at a time, its outcome shared with everyone who
//! asked while it ran.
//!
//! The mechanism of the token cache (`token_provider/flight.rs`, audit C3),
//! without the token: the first caller to find nothing running becomes the
//! *leader*, publishes a [`watch`] channel as the run in flight, runs the
//! operation and sends the outcome, success or failure, to everyone waiting
//! on it. A mutex around the operation would collapse concurrent successes
//! only: each caller queued behind a failure would run its own attempt, one
//! after another.
//!
//! Used by the card resolver (ADR 0051: N callers holding a stale card make
//! one fetch) and the balancer's endpoint refresh (ADR 0050).
//!
//! # Cancellation
//!
//! No lock is held across an `.await`. A waiter that is dropped drops its
//! receiver. A leader that is dropped mid-run drops a guard that clears the
//! slot and sends [`Outcome::Abandoned`]; its waiters look again, and one of
//! them leads the next run. A leader that panics unwinds through the same
//! guard.

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::watch;

use crate::error::{ClientError, ClientResult};
use crate::token_provider::flight::replicate;

/// What a run ended in, as its waiters see it.
#[derive(Clone)]
enum Outcome<T> {
    Pending,
    Done(Result<T, Arc<ClientError>>),
    Abandoned,
}

/// The run in flight, if any.
pub struct SingleFlight<T> {
    slot: Mutex<Option<Arc<watch::Sender<Outcome<T>>>>>,
}

impl<T> Default for SingleFlight<T> {
    fn default() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }
}

impl<T> std::fmt::Debug for SingleFlight<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingleFlight")
            .field(
                "running",
                &self
                    .slot
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_some(),
            )
            .finish()
    }
}

/// What a caller does after looking at the slot.
enum Role<T> {
    Wait(watch::Receiver<Outcome<T>>),
    Lead(Arc<watch::Sender<Outcome<T>>>),
}

impl<T: Clone + Send + Sync> SingleFlight<T> {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Arc<watch::Sender<Outcome<T>>>>> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Joins the run in flight, or starts one with this caller as leader.
    fn claim(&self) -> Role<T> {
        let mut slot = self.lock();
        if let Some(tx) = slot.as_ref() {
            let rx = tx.subscribe();
            drop(slot);
            return Role::Wait(rx);
        }
        let tx = Arc::new(watch::Sender::new(Outcome::Pending));
        *slot = Some(Arc::clone(&tx));
        drop(slot);
        Role::Lead(tx)
    }

    /// Runs `op`, or waits for the run already in flight and returns its
    /// outcome. An error reaches each waiter as a copy (`replicate`), with
    /// the same retry classification.
    pub async fn run<F, Fut>(&self, op: F) -> ClientResult<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = ClientResult<T>>,
    {
        loop {
            let role = self.claim();
            let mut rx = match role {
                Role::Lead(tx) => {
                    let leader = Leader {
                        flight: self,
                        tx: Some(tx),
                    };
                    return leader.finish(op().await);
                }
                Role::Wait(rx) => rx,
            };
            let outcome = rx
                .wait_for(|o| !matches!(o, Outcome::Pending))
                .await
                .map_or(Outcome::Abandoned, |o| o.clone());
            match outcome {
                Outcome::Done(Ok(value)) => return Ok(value),
                Outcome::Done(Err(e)) => return Err(replicate(&e)),
                // The leader went away without an answer: look again.
                Outcome::Pending | Outcome::Abandoned => {}
            }
        }
    }

    /// How many callers wait on the run in flight, for tests.
    #[cfg(test)]
    pub fn waiters(&self) -> usize {
        self.lock().as_ref().map_or(0, |tx| tx.receiver_count())
    }
}

/// The caller running the operation. Dropped before [`Leader::finish`], it
/// marks the run abandoned.
struct Leader<'a, T: Clone + Send + Sync> {
    flight: &'a SingleFlight<T>,
    tx: Option<Arc<watch::Sender<Outcome<T>>>>,
}

impl<T: Clone + Send + Sync> Leader<'_, T> {
    fn finish(mut self, result: ClientResult<T>) -> ClientResult<T> {
        *self.flight.lock() = None;
        let (shared, returned) = match result {
            Ok(value) => (Ok(value.clone()), Ok(value)),
            Err(e) => (Err(Arc::new(replicate(&e))), Err(e)),
        };
        if let Some(tx) = self.tx.take() {
            // `send_replace`: with no receiver `send` would not store it.
            tx.send_replace(Outcome::Done(shared));
        }
        returned
    }
}

impl<T: Clone + Send + Sync> Drop for Leader<'_, T> {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            *self.flight.lock() = None;
            tx.send_replace(Outcome::Abandoned);
        }
    }
}

#[cfg(test)]
mod tests;
