// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ModelRetry`]: sending a rate-limited turn again, after the provider's
//! `Retry-After`, inside the caller's budget (audit H17, ADR 0048).

use std::time::{Duration, SystemTime};

use a2a_protocol_types::budget::BudgetHandle;

/// The longest `Retry-After` read from a provider. A header asking for more
/// is read as this, and then refused by any [`ModelRetry::max_delay`] below
/// it, so a hostile or broken header cannot park a turn. The client crate
/// clamps the same header to the same hour.
const RETRY_AFTER_CEILING: Duration = Duration::from_secs(3600);

/// How a loop retries a turn the provider refused as rate limited
/// ([`ModelError::RateLimited`](super::ModelError::RateLimited)); set with
/// [`ToolLoop::with_model_retry`](super::ToolLoop::with_model_retry).
///
/// No other failure is retried. Each wait is:
///
/// * the provider's `Retry-After`, lengthened by up to a quarter at random,
///   so loops refused together do not come back together, and never more
///   than [`max_delay`](Self::max_delay); a `Retry-After` longer than
///   `max_delay` is not waited for, and the run fails with the 429;
/// * with no `Retry-After`, `base_delay` doubled for each retry already made,
///   at most `max_delay`, and then between half and all of that at random.
///
/// A wait longer than the budget's remaining time (ADR 0023) is not made
/// either: the run fails with the 429 then, rather than after the deadline.
///
/// ```rust
/// use std::time::Duration;
/// use a2a_protocol_server::tool_loop::ModelRetry;
///
/// let retry = ModelRetry::new()
///     .with_max_retries(5)
///     .with_max_delay(Duration::from_secs(10));
/// assert_eq!(retry.max_retries(), 5);
/// assert_eq!(retry.base_delay(), Duration::from_millis(500));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelRetry {
    max_retries: u32,
    base_delay: Duration,
    max_delay: Duration,
}

impl ModelRetry {
    /// Three retries, from 500 ms, waiting at most 30 s each.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }

    /// Sets how many times one turn may be sent again.
    #[must_use]
    pub const fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Sets the first wait when the provider gave no `Retry-After`.
    #[must_use]
    pub const fn with_base_delay(mut self, base_delay: Duration) -> Self {
        self.base_delay = base_delay;
        self
    }

    /// Sets the longest wait.
    #[must_use]
    pub const fn with_max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
        self
    }

    /// How many times one turn may be sent again.
    #[must_use]
    pub const fn max_retries(&self) -> u32 {
        self.max_retries
    }

    /// The first wait when the provider gave no `Retry-After`.
    #[must_use]
    pub const fn base_delay(&self) -> Duration {
        self.base_delay
    }

    /// The longest wait.
    #[must_use]
    pub const fn max_delay(&self) -> Duration {
        self.max_delay
    }

    /// The wait before retry number `retries + 1`, or `None` when the turn
    /// is not to be sent again: the retries are spent, the provider asked
    /// for longer than `max_delay`, or the budget ends before the wait
    /// would.
    pub(super) fn next_wait(
        &self,
        retries: u32,
        retry_after: Option<Duration>,
        budget: Option<&BudgetHandle>,
    ) -> Option<Duration> {
        let wait = self.wait_for(retries, retry_after, random_bits())?;
        // Not a wait that ends at the deadline either: the turn would start
        // with nothing left of the budget.
        let left = budget.and_then(BudgetHandle::remaining_time);
        if left.is_some_and(|left| wait >= left) {
            return None;
        }
        Some(wait)
    }

    /// [`next_wait`](Self::next_wait) without the budget, with the random
    /// draw given, so the arithmetic is tested exactly.
    fn wait_for(&self, retries: u32, retry_after: Option<Duration>, bits: u64) -> Option<Duration> {
        if retries >= self.max_retries {
            return None;
        }
        if let Some(after) = retry_after {
            if after > self.max_delay {
                return None;
            }
            let spread = portion(after / 4, draw(bits));
            return Some(after.saturating_add(spread).min(self.max_delay));
        }
        let doubled = self
            .base_delay
            .checked_mul(1u32.checked_shl(retries).unwrap_or(u32::MAX))
            .unwrap_or(self.max_delay)
            .min(self.max_delay);
        let half = doubled / 2;
        Some(half + portion(doubled.saturating_sub(half), draw(bits)))
    }
}

impl Default for ModelRetry {
    /// [`ModelRetry::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// The high 32 bits of a 64-bit draw.
const fn draw(bits: u64) -> u32 {
    let [a, b, c, d, ..] = bits.to_be_bytes();
    u32::from_be_bytes([a, b, c, d])
}

/// `draw / u32::MAX` of `d`, rounded down to the nanosecond: none of it at
/// a draw of `0`, all of it at `u32::MAX`. Integer arithmetic, so the
/// jitter is exact and needs no float cast. The product cannot overflow:
/// `Duration::MAX` is under 2^94 nanoseconds and the draw under 2^32.
fn portion(d: Duration, draw: u32) -> Duration {
    let nanos = d.as_nanos() * u128::from(draw) / u128::from(u32::MAX);
    u64::try_from(nanos).map_or(d, Duration::from_nanos)
}

/// A random draw, from the standard library's per-process hasher keys, as
/// the client's backoff jitter draws one.
fn random_bits() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u8(0);
    hasher.finish()
}

/// A `Retry-After` header as a delay: delta-seconds, or the time until an
/// HTTP date (zero once it has passed), at most an hour; `None` when absent
/// or unreadable (RFC 9110 §10.2.3).
pub(super) fn parse_retry_after(headers: &hyper::HeaderMap, now: SystemTime) -> Option<Duration> {
    let raw = headers
        .get(hyper::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    let delay = match raw.parse::<u64>() {
        Ok(secs) => Duration::from_secs(secs),
        Err(_) => httpdate::parse_http_date(raw)
            .ok()?
            .duration_since(now)
            .unwrap_or(Duration::ZERO),
    };
    Some(delay.min(RETRY_AFTER_CEILING))
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod tests;
