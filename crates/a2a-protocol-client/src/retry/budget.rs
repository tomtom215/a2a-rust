// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A budget for retries: retries as a bounded fraction of calls (audit H6).
//!
//! A client that retries multiplies the load on a peer that is already
//! refusing it: with three retries, a saturated worker sees up to four times
//! the calls it was sent. A [`RetryBudget`] bounds that. Each call earns the
//! client a fraction of a retry, time earns a little more, and a retry spends
//! one; a retry the budget cannot pay for is not made, and the error that
//! called for it is returned.
//!
//! The budget is per client (per `ClientBuilder::build`). A thousand clients
//! in one process each have their own, so the amplification towards one peer
//! across clients is not bounded by this; a budget shared by the clients of a
//! pool is the pool's business (ADR 0032).

use std::sync::{Mutex, PoisonError};

use tokio::time::Instant;

/// How many retries a client may make, as a fraction of the calls it makes.
///
/// `RetryBudget::new(0.1, 1.0)` allows one retry for every ten calls, and one
/// a second besides, so that a client that makes a call a minute can still
/// retry it. Up to [`reserve`](Self::reserve) retries are saved up for a burst
/// of failures.
///
/// `#[non_exhaustive]`: build it with [`new`](Self::new) and
/// [`with_reserve`](Self::with_reserve).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct RetryBudget {
    /// Retries earned by each call. `0.1` is one retry per ten calls.
    pub ratio: f64,
    /// Retries earned per second, whatever the calls, so that a client that
    /// makes few can still retry them.
    pub min_per_sec: f64,
    /// The most retries that can be saved up, and the number a new client
    /// starts with: the size of the burst it can make before the rate above
    /// is all there is. At least `1.0`, since less could never pay for one.
    pub reserve: f64,
}

/// The default [`RetryBudget::reserve`].
pub const DEFAULT_RESERVE: f64 = 10.0;

/// A rate that is not a finite, positive number earns nothing. (`max` and not a
/// comparison: zero is zero either way, so a `>` against zero has no behaviour
/// for a test to tell from `>=`.)
const fn rate(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

impl RetryBudget {
    /// A budget earning `ratio` retries per call and `min_per_sec` per second,
    /// with a reserve of 10. A rate that is negative,
    /// infinite or not a number earns nothing.
    #[must_use]
    pub const fn new(ratio: f64, min_per_sec: f64) -> Self {
        Self {
            ratio: rate(ratio),
            min_per_sec: rate(min_per_sec),
            reserve: DEFAULT_RESERVE,
        }
    }

    /// Sets the most retries that can be saved up. Less than `1.0`, or not a
    /// number, becomes `1.0`: a retry costs one.
    #[must_use]
    pub const fn with_reserve(mut self, reserve: f64) -> Self {
        self.reserve = if reserve.is_nan() {
            1.0
        } else {
            reserve.clamp(1.0, f64::MAX)
        };
        self
    }
}

/// What the budget has earned and spent. Fractions of a retry are summed in
/// floating point, so a retry is affordable from `1 - EPSILON`: ten calls at
/// `0.1` must pay for one although their sum is `0.9999999999999999`.
const EPSILON: f64 = 1e-9;

struct State {
    tokens: f64,
    /// The instant up to which time has been counted.
    at: Instant,
}

/// A [`RetryBudget`] with its balance.
pub struct Ledger {
    budget: RetryBudget,
    state: Mutex<State>,
}

impl Ledger {
    pub(crate) fn new(budget: RetryBudget) -> Self {
        Self {
            budget,
            state: Mutex::new(State {
                tokens: budget.reserve,
                at: Instant::now(),
            }),
        }
    }

    /// Counts the time that has passed since the last call into the balance.
    fn earn_time(&self, state: &mut State) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(state.at).as_secs_f64();
        state.tokens = elapsed
            .mul_add(self.budget.min_per_sec, state.tokens)
            .min(self.budget.reserve);
        state.at = now;
    }

    /// A call was made: it earns its fraction of a retry.
    pub(crate) fn deposit(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        self.earn_time(&mut state);
        state.tokens = (state.tokens + self.budget.ratio).min(self.budget.reserve);
    }

    /// Gives back one retry spent on a retry that was not made.
    pub(crate) fn refund(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.tokens = (state.tokens + 1.0).min(self.budget.reserve);
    }

    /// Spends one retry if there is one to spend.
    pub(crate) fn withdraw(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        self.earn_time(&mut state);
        if state.tokens >= 1.0 - EPSILON {
            state.tokens = (state.tokens - 1.0).max(0.0);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests;
