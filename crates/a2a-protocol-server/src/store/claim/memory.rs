// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`TurnClaimStore`] in process memory: for tests that run several handlers
//! over one in-memory store.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use super::{Fut, Handed, HeldClaim, PendingSteering, TurnClaimStore};

/// Claims in process memory, for tests that run several handlers over one
/// in-memory store. Expiry is measured on this process's monotonic clock.
///
/// Steering messages handed to a claim ([`TurnClaimStore::hand_steering`])
/// are kept here too, in process memory: this store is only as shared as the
/// handlers holding it, which is why it is for tests and not for replicas.
#[derive(Debug, Default)]
pub struct InMemoryTurnClaims {
    claims: Mutex<HashMap<(String, String), Entry>>,
    /// Steering messages, oldest first.
    steering: Mutex<Vec<Queued>>,
    next_seq: AtomicI64,
}

#[derive(Debug)]
struct Entry {
    owner: String,
    token: String,
    cancel_requested: bool,
    expires: Instant,
}

#[derive(Debug)]
struct Queued {
    seq: i64,
    token: String,
    message: String,
}

impl InMemoryTurnClaims {
    /// No claims.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `token` still holds a claim.
    #[must_use]
    pub fn holds(&self, token: &str) -> bool {
        self.lock().values().any(|e| e.token == token)
    }

    /// How many steering messages are stored, for any claim: what a test
    /// reads to see that a refused message left nothing behind.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.queue().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), Entry>> {
        self.claims
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn queue(&self) -> std::sync::MutexGuard<'_, Vec<Queued>> {
        self.steering
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl TurnClaimStore for InMemoryTurnClaims {
    fn claim<'a>(
        &'a self,
        scope: &'a str,
        task_id: &'a str,
        owner: &'a str,
        token: &'a str,
        ttl: Duration,
    ) -> Fut<'a, bool> {
        Box::pin(async move {
            let now = Instant::now();
            let entry = Entry {
                owner: owner.to_owned(),
                token: token.to_owned(),
                cancel_requested: false,
                expires: now + ttl,
            };
            let key = (scope.to_owned(), task_id.to_owned());
            let mut claims = self.lock();
            if claims.get(&key).is_some_and(|e| live(e.expires, now)) {
                return Ok(false);
            }
            claims.insert(key, entry);
            drop(claims);
            Ok(true)
        })
    }

    fn release<'a>(&'a self, token: &'a str) -> Fut<'a, ()> {
        Box::pin(async move {
            self.lock().retain(|_, e| e.token != token);
            self.queue().retain(|q| q.token != token);
            Ok(())
        })
    }

    fn renew<'a>(
        &'a self,
        owner: &'a str,
        tokens: &'a [String],
        ttl: Duration,
    ) -> Fut<'a, Vec<HeldClaim>> {
        Box::pin(async move {
            let expires = Instant::now() + ttl;
            Ok(self
                .lock()
                .values_mut()
                .filter(|e| e.owner == owner && tokens.contains(&e.token))
                .map(|e| {
                    e.expires = expires;
                    HeldClaim {
                        token: e.token.clone(),
                        cancel_requested: e.cancel_requested,
                    }
                })
                .collect())
        })
    }

    fn request_cancel<'a>(&'a self, scope: &'a str, task_id: &'a str) -> Fut<'a, bool> {
        Box::pin(async move {
            let now = Instant::now();
            Ok(self
                .lock()
                .get_mut(&(scope.to_owned(), task_id.to_owned()))
                .filter(|e| live(e.expires, now))
                .map(|e| e.cancel_requested = true)
                .is_some())
        })
    }

    fn sweep(&self, older_than: Duration) -> Fut<'_, u64> {
        Box::pin(async move {
            let now = Instant::now();
            let mut claims = self.lock();
            let before = claims.len();
            claims.retain(|_, e| !swept(e.expires, older_than, now));
            Ok((before - claims.len()) as u64)
        })
    }

    fn hand_steering<'a>(
        &'a self,
        scope: &'a str,
        task_id: &'a str,
        message: &'a str,
        capacity: usize,
    ) -> Fut<'a, Handed> {
        Box::pin(async move {
            let now = Instant::now();
            let token = self
                .lock()
                .get(&(scope.to_owned(), task_id.to_owned()))
                .filter(|e| live(e.expires, now))
                .map(|e| e.token.clone());
            let Some(token) = token else {
                return Ok(Handed::NoClaim);
            };
            let mut queue = self.queue();
            if queue.iter().filter(|q| q.token == token).count() >= capacity {
                return Ok(Handed::Full);
            }
            let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
            queue.push(Queued {
                seq,
                token,
                message: message.to_owned(),
            });
            drop(queue);
            Ok(Handed::Queued)
        })
    }

    fn peek_steering<'a>(
        &'a self,
        owner: &'a str,
        tokens: &'a [String],
        limit: usize,
    ) -> Fut<'a, Vec<PendingSteering>> {
        Box::pin(async move {
            let now = Instant::now();
            let held: Vec<String> = self
                .lock()
                .values()
                .filter(|e| e.owner == owner && live(e.expires, now) && tokens.contains(&e.token))
                .map(|e| e.token.clone())
                .collect();
            Ok(self
                .queue()
                .iter()
                .filter(|q| held.contains(&q.token))
                .take(limit)
                .map(|q| PendingSteering {
                    seq: q.seq,
                    token: q.token.clone(),
                    message: q.message.clone(),
                })
                .collect())
        })
    }

    fn ack_steering<'a>(&'a self, seqs: &'a [i64]) -> Fut<'a, ()> {
        Box::pin(async move {
            self.queue().retain(|q| !seqs.contains(&q.seq));
            Ok(())
        })
    }
}

/// Whether a claim expiring at `expires` still holds at `now`: until the
/// instant it expires, and not at it, which is when another turn may take it.
pub(super) fn live(expires: Instant, now: Instant) -> bool {
    expires > now
}

/// Whether a claim that expired at `expires` is past the sweep's grace of
/// `older_than` at `now`: swept at the instant the grace ends, not before.
pub(super) fn swept(expires: Instant, older_than: Duration, now: Instant) -> bool {
    expires + older_than <= now
}
