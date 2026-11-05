// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The arithmetic of [`ModelRetry`]'s waits, with the random draw fixed, and
//! the `Retry-After` reader against a fixed clock.

use std::time::{Duration, Instant, SystemTime};

use a2a_protocol_types::budget::{Budget, BudgetHandle};
use hyper::HeaderMap;

use super::*;

const MS: fn(u64) -> Duration = Duration::from_millis;

/// The provider's delay is a floor, spread by up to a quarter: exactly the
/// delay at the lowest draw, a quarter more at the highest, an eighth more
/// half way.
#[test]
fn a_retry_after_is_waited_and_spread_by_up_to_a_quarter() {
    let retry = ModelRetry::new();
    assert_eq!(retry.wait_for(0, Some(MS(2000)), 0), Some(MS(2000)));
    assert_eq!(retry.wait_for(0, Some(MS(2000)), u64::MAX), Some(MS(2500)));
    assert_eq!(retry.wait_for(0, Some(MS(2000)), 1 << 63), Some(MS(2250)));
}

/// A delay equal to `max_delay` is waited, its spread cut at `max_delay`; one
/// past it is not waited at all.
#[test]
fn a_retry_after_past_the_longest_wait_is_not_waited_for() {
    let retry = ModelRetry::new().with_max_delay(MS(2000));
    assert_eq!(retry.wait_for(0, Some(MS(2000)), u64::MAX), Some(MS(2000)));
    assert_eq!(retry.wait_for(0, Some(MS(2001)), 0), None);
}

/// With no `Retry-After`: `base_delay` doubled per retry made, capped at
/// `max_delay`, then between half and all of that.
#[test]
fn without_a_retry_after_the_wait_doubles_and_is_jittered_by_half() {
    let retry = ModelRetry::new().with_max_retries(40);
    assert_eq!(retry.wait_for(0, None, 0), Some(MS(250)));
    assert_eq!(retry.wait_for(0, None, u64::MAX), Some(MS(500)));
    assert_eq!(retry.wait_for(1, None, u64::MAX), Some(MS(1000)));
    assert_eq!(retry.wait_for(2, None, u64::MAX), Some(MS(2000)));
    // 500 ms × 2^6 = 32 s, past the 30 s cap.
    assert_eq!(retry.wait_for(6, None, u64::MAX), Some(MS(30_000)));
    assert_eq!(retry.wait_for(6, None, 0), Some(MS(15_000)));
    // A shift past the width of the multiplier, and a product past what a
    // duration holds, are both the cap.
    assert_eq!(retry.wait_for(35, None, u64::MAX), Some(MS(30_000)));
    let huge = ModelRetry::new()
        .with_max_retries(40)
        .with_base_delay(Duration::MAX)
        .with_max_delay(Duration::from_secs(7));
    assert_eq!(
        huge.wait_for(1, None, u64::MAX),
        Some(Duration::from_secs(7))
    );
}

/// With no cap a wait may be as long as a duration holds: the jitter is at
/// most the half it spreads over, so the sum reaches `Duration::MAX` and does
/// not overflow. Past 2^64 nanoseconds (584 years) the jitter is the whole
/// half at any draw.
#[test]
fn an_uncapped_wait_reaches_the_longest_duration() {
    let unbounded = ModelRetry::new()
        .with_base_delay(Duration::MAX)
        .with_max_delay(Duration::MAX);
    assert_eq!(unbounded.wait_for(0, None, 0), Some(Duration::MAX / 2));
    assert_eq!(unbounded.wait_for(0, None, u64::MAX), Some(Duration::MAX));
    assert_eq!(unbounded.wait_for(0, None, 1 << 63), Some(Duration::MAX));
}

/// `max_retries` retries, then none.
#[test]
fn the_retries_end_at_max_retries() {
    let retry = ModelRetry::new().with_max_retries(2);
    assert_eq!(retry.wait_for(1, Some(MS(10)), 0), Some(MS(10)));
    assert_eq!(retry.wait_for(2, Some(MS(10)), 0), None);
    assert_eq!(
        ModelRetry::new().with_max_retries(0).wait_for(0, None, 0),
        None
    );
}

/// A wait the budget cannot cover is not made, and neither is one that
/// would end exactly at the deadline: the turn would start with nothing
/// left. A budget with room, or none, does not shorten the wait.
#[test]
fn a_wait_the_budget_cannot_cover_is_not_made() {
    let retry = ModelRetry::new();
    let roomy = BudgetHandle::start(&Budget::new().with_time(Duration::from_secs(3600)));
    assert_eq!(retry.next_wait(0, Some(MS(0)), Some(&roomy)), Some(MS(0)));
    assert_eq!(retry.next_wait(0, Some(MS(0)), None), Some(MS(0)));
    let short = BudgetHandle::start(&Budget::new().with_time(MS(1000)));
    assert_eq!(retry.next_wait(0, Some(MS(5000)), Some(&short)), None);
    let spent = BudgetHandle::start_at(
        &Budget::new().with_time(MS(1)),
        Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
    );
    assert_eq!(spent.remaining_time(), Some(Duration::ZERO));
    assert_eq!(retry.next_wait(0, Some(MS(0)), Some(&spent)), None);
    // A budget with meters and no deadline does not bound the wait.
    let untimed = BudgetHandle::start(&Budget::new().with_meter("tokens", 10).unwrap());
    assert_eq!(retry.next_wait(0, Some(MS(0)), Some(&untimed)), Some(MS(0)));
}

/// The defaults, as documented, and `Default` is `new`.
#[test]
fn the_defaults_are_three_retries_from_half_a_second_to_thirty() {
    let retry = ModelRetry::default();
    assert_eq!(retry, ModelRetry::new());
    assert_eq!(retry.max_retries(), 3);
    assert_eq!(retry.base_delay(), MS(500));
    assert_eq!(retry.max_delay(), Duration::from_secs(30));
    let set = ModelRetry::new()
        .with_max_retries(9)
        .with_base_delay(MS(7))
        .with_max_delay(MS(70));
    assert_eq!(
        (set.max_retries(), set.base_delay(), set.max_delay()),
        (9, MS(7), MS(70))
    );
    assert_eq!(
        format!("{set:?}"),
        "ModelRetry { max_retries: 9, base_delay: 7ms, max_delay: 70ms }"
    );
}

/// Two draws in a row are not the same number: the jitter is random, not a
/// constant (a constant would bring refused loops back together).
#[test]
fn the_draw_is_random() {
    let draws: std::collections::BTreeSet<u64> = (0..64).map(|_| random_bits()).collect();
    assert!(draws.len() > 32, "{} distinct of 64", draws.len());
}

fn headers(value: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(hyper::header::RETRY_AFTER, value.parse().unwrap());
    h
}

/// Delta-seconds, an HTTP date ahead of or behind the clock, the hour cap,
/// and what is not a `Retry-After`.
#[test]
fn a_retry_after_is_read_as_seconds_or_as_a_date() {
    let now = httpdate::parse_http_date("Wed, 04 Nov 2026 10:00:00 GMT").unwrap();
    assert_eq!(parse_retry_after(&headers("2"), now), Some(MS(2000)));
    assert_eq!(parse_retry_after(&headers(" 7 "), now), Some(MS(7000)));
    assert_eq!(
        parse_retry_after(&headers("Wed, 04 Nov 2026 10:00:10 GMT"), now),
        Some(MS(10_000))
    );
    assert_eq!(
        parse_retry_after(&headers("Wed, 04 Nov 2026 09:59:00 GMT"), now),
        Some(Duration::ZERO)
    );
    assert_eq!(
        parse_retry_after(&headers("99999"), now),
        Some(Duration::from_secs(3600))
    );
    assert_eq!(
        parse_retry_after(&headers("3600"), now),
        Some(Duration::from_secs(3600))
    );
    assert_eq!(parse_retry_after(&headers("soon"), now), None);
    assert_eq!(parse_retry_after(&HeaderMap::new(), now), None);
    let mut opaque = HeaderMap::new();
    opaque.insert(
        hyper::header::RETRY_AFTER,
        hyper::header::HeaderValue::from_bytes(b"\xff").unwrap(),
    );
    assert_eq!(parse_retry_after(&opaque, SystemTime::UNIX_EPOCH), None);
}
