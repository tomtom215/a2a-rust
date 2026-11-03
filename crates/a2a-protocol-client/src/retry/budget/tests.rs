// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The budget's arithmetic, in paused time so that each boundary is exact.

use std::time::Duration;

use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// How many retries the ledger will pay for right now.
fn affordable(ledger: &Ledger) -> u32 {
    let mut n = 0;
    while ledger.withdraw() {
        n += 1;
        assert!(n < 10_000, "a ledger that never runs dry");
    }
    n
}

#[test]
fn a_rate_that_is_not_a_positive_number_earns_nothing() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0] {
        let b = RetryBudget::new(bad, bad);
        assert_eq!((b.ratio, b.min_per_sec), (0.0, 0.0), "{bad}");
    }
    let b = RetryBudget::new(0.1, 2.5);
    assert_eq!((b.ratio, b.min_per_sec, b.reserve), (0.1, 2.5, 10.0));
    assert_eq!(DEFAULT_RESERVE, 10.0);
}

#[test]
fn a_reserve_is_at_least_one_retry_and_always_a_number() {
    let reserve = |r: f64| RetryBudget::new(0.1, 1.0).with_reserve(r).reserve;
    assert_eq!(reserve(25.0), 25.0);
    assert_eq!(reserve(1.0), 1.0);
    assert_eq!(reserve(0.99), 1.0);
    assert_eq!(reserve(-3.0), 1.0);
    assert_eq!(reserve(f64::NAN), 1.0);
    assert_eq!(reserve(f64::INFINITY), f64::MAX);
    assert_eq!(reserve(f64::NEG_INFINITY), 1.0);
}

#[tokio::test(start_paused = true)]
async fn a_new_client_may_retry_up_to_its_reserve_and_no_further() {
    let ledger = Ledger::new(RetryBudget::new(0.0, 0.0));
    assert_eq!(affordable(&ledger), 10);
    assert!(!ledger.withdraw(), "and it stays empty");
    let ledger = Ledger::new(RetryBudget::new(0.0, 0.0).with_reserve(3.0));
    assert_eq!(affordable(&ledger), 3);
}

#[tokio::test(start_paused = true)]
async fn calls_earn_their_fraction_of_a_retry_without_floating_point_drift() {
    let ledger = Ledger::new(RetryBudget::new(0.1, 0.0));
    affordable(&ledger);
    for _ in 0..9 {
        ledger.deposit();
    }
    assert!(!ledger.withdraw(), "nine tenths of a retry");
    ledger.deposit();
    assert!(ledger.withdraw(), "ten calls pay for one retry");
    assert!(!ledger.withdraw(), "and for only one");
}

#[tokio::test(start_paused = true)]
async fn time_earns_retries_at_the_stated_rate_to_the_millisecond() {
    let ledger = Ledger::new(RetryBudget::new(0.0, 1.0));
    affordable(&ledger);
    tokio::time::advance(ms(999)).await;
    assert!(!ledger.withdraw(), "a millisecond short of one");
    tokio::time::advance(ms(1)).await;
    assert!(ledger.withdraw(), "one a second");
    assert!(!ledger.withdraw());

    // A rate below one a second still accrues across calls that each earn less
    // than a retry.
    let ledger = Ledger::new(RetryBudget::new(0.0, 0.5));
    affordable(&ledger);
    for _ in 0..3 {
        tokio::time::advance(ms(500)).await;
        assert!(!ledger.withdraw());
    }
    tokio::time::advance(ms(500)).await;
    assert!(ledger.withdraw(), "two seconds at half a retry a second");
}

#[tokio::test(start_paused = true)]
async fn nothing_is_saved_up_beyond_the_reserve() {
    let ledger = Ledger::new(RetryBudget::new(1.0, 1.0).with_reserve(4.0));
    tokio::time::advance(Duration::from_secs(3600)).await;
    assert_eq!(affordable(&ledger), 4, "an idle hour");
    for _ in 0..1_000 {
        ledger.deposit();
    }
    assert_eq!(affordable(&ledger), 4, "a thousand calls");
}

#[tokio::test(start_paused = true)]
async fn a_budget_that_earns_nothing_is_spent_for_good() {
    let ledger = Ledger::new(RetryBudget::new(0.0, 0.0).with_reserve(1.0));
    assert!(ledger.withdraw());
    for _ in 0..100 {
        ledger.deposit();
        tokio::time::advance(Duration::from_secs(10)).await;
    }
    assert!(!ledger.withdraw());
}
