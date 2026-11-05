// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! When a stored card may be used after its revalidation failed (RFC 5861
//! §4, stale-if-error), and the jitter that spreads revalidations (ADR 0051).

use std::time::Duration;

use tokio::time::Instant;

use crate::error::ClientError;

/// The error text of a card fetch whose response headers did not arrive in
/// its budget.
pub(super) const FETCH_TIMED_OUT: &str = "agent card fetch timed out";

/// The error text of a card fetch whose body did not arrive in its budget.
pub(super) const BODY_TIMED_OUT: &str = "agent card body read timed out";

/// The most a card's freshness is shortened by, as a share of its lifetime.
const MAX_JITTER: f64 = 0.5;

/// Whether `error` says the agent could not be reached or could not answer,
/// which is what stale-if-error covers: RFC 5861 §4 names "a 500, 502, 503
/// or 504", and a connection that failed or a fetch that timed out is the
/// same want of a server. A card that was refused (`404`), did not parse or
/// did not verify is an answer, and is never covered.
pub(super) fn unavailable(error: &ClientError) -> bool {
    match error {
        ClientError::Http(_) | ClientError::HttpClient(_) | ClientError::Timeout(_) => true,
        ClientError::UnexpectedStatus { status, .. } => matches!(status, 500 | 502 | 503 | 504),
        ClientError::Transport(text) => text == FETCH_TIMED_OUT || text == BODY_TIMED_OUT,
        _ => false,
    }
}

/// Whether a response stale since `since` is stale by no more than `window`
/// at `now`.
pub(super) fn within(since: Instant, window: Duration, now: Instant) -> bool {
    now.saturating_duration_since(since) <= window
}

/// `fraction`, clamped to `0.0..=0.5`; not a number is `0.0`.
pub(super) const fn clamp_jitter(fraction: f64) -> f64 {
    if fraction.is_nan() {
        0.0
    } else {
        fraction.clamp(0.0, MAX_JITTER)
    }
}

/// The share of a card's lifetime its freshness is shortened by: `fraction`
/// scaled by `bits`, a uniform 64-bit draw (its top 32 bits, as
/// `pool::generations` reads one).
pub(super) fn jitter_share(fraction: f64, bits: u64) -> f64 {
    let top = u32::try_from(bits >> 32).unwrap_or(u32::MAX);
    fraction * (f64::from(top) / f64::from(u32::MAX))
}

/// A 64-bit draw. `RandomState` is seeded from the operating system, which
/// is all a spread needs: nothing here is secret.
pub(super) fn random_bits() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_counts_as_the_agent_being_unavailable() {
        for status in [500, 502, 503, 504] {
            let e = ClientError::UnexpectedStatus {
                status,
                body: String::new(),
                retry_after: None,
            };
            assert!(unavailable(&e), "{status}");
        }
        for status in [404, 410, 429, 501, 505] {
            let e = ClientError::UnexpectedStatus {
                status,
                body: String::new(),
                retry_after: None,
            };
            assert!(!unavailable(&e), "{status}");
        }
        assert!(unavailable(&ClientError::HttpClient("refused".into())));
        assert!(unavailable(&ClientError::Timeout("t".into())));
        assert!(unavailable(&ClientError::Transport(FETCH_TIMED_OUT.into())));
        assert!(unavailable(&ClientError::Transport(BODY_TIMED_OUT.into())));
        assert!(!unavailable(&ClientError::Transport("too large".into())));
        assert!(!unavailable(&ClientError::InvalidAgentResponse("x".into())));
    }

    #[test]
    fn the_window_includes_its_last_instant() {
        let since = Instant::now();
        let window = Duration::from_secs(10);
        assert!(within(since, window, since));
        assert!(within(since, window, since + window));
        assert!(!within(
            since,
            window,
            since + window + Duration::from_nanos(1)
        ));
        // A `since` after `now` is not stale at all.
        assert!(within(since + window, window, since));
    }

    #[test]
    fn jitter_is_clamped() {
        assert!((clamp_jitter(0.1) - 0.1).abs() < f64::EPSILON);
        assert!(clamp_jitter(-1.0).abs() < f64::EPSILON);
        assert!((clamp_jitter(0.9) - 0.5).abs() < f64::EPSILON);
        assert!(clamp_jitter(f64::NAN).abs() < f64::EPSILON);
    }

    #[test]
    fn the_share_scales_the_fraction_by_the_draw() {
        assert!(jitter_share(0.1, 0).abs() < f64::EPSILON);
        assert!((jitter_share(0.1, u64::MAX) - 0.1).abs() < 1e-12);
        let half = u64::from(u32::MAX / 2) << 32;
        assert!((jitter_share(0.2, half) - 0.1).abs() < 1e-9);
    }
}
