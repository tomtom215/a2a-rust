// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What stops a retry besides running out of them: a server-directed delay
//! longer than `max_backoff` (audit D-10), `max_elapsed`, and the budget
//! (audit H6). Paused time, so each bound is exact.
//!
//! A `Retry-After` of exactly `max_backoff` makes the wait exact too: the
//! delay is that value spread upward and clamped to it, so it is `max_backoff`
//! whatever the spread drew.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

const MAX_BACKOFF: Duration = Duration::from_secs(30);

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// One nanosecond less than `d`.
fn just_under(d: Duration) -> Duration {
    d.saturating_sub(Duration::from_nanos(1))
}

/// Fails every call with the error `make` builds, counting the calls.
struct Failing {
    make: fn() -> ClientError,
    calls: Arc<AtomicUsize>,
}

impl Failing {
    fn new(make: fn() -> ClientError) -> (Self, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Self {
                make,
                calls: Arc::clone(&calls),
            },
            calls,
        )
    }
}

impl Transport for Failing {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let error = (self.make)();
        Box::pin(async move { Err(error) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let error = (self.make)();
        Box::pin(async move { Err(error) })
    }
}

fn timeout() -> ClientError {
    ClientError::Timeout("transient".into())
}

fn asked_to_wait(after: Duration) -> ClientError {
    ClientError::UnexpectedStatus {
        status: 503,
        body: String::new(),
        retry_after: Some(after),
    }
}

fn exactly_max_backoff() -> ClientError {
    asked_to_wait(MAX_BACKOFF)
}

fn a_second_over_max_backoff() -> ClientError {
    asked_to_wait(MAX_BACKOFF + secs(1))
}

fn policy(retries: u32) -> RetryPolicy {
    RetryPolicy::default()
        .with_max_retries(retries)
        .with_initial_backoff(Duration::from_millis(10))
        .with_backoff_multiplier(1.0)
        .with_max_backoff(MAX_BACKOFF)
}

/// One unary call: how many attempts it made and how long it took.
async fn unary(transport: &RetryTransport) -> (ClientError, Duration) {
    let started = Instant::now();
    let error = transport
        .send_request("GetTask", serde_json::json!({}), &HashMap::new())
        .await
        .expect_err("the transport always fails");
    (error, started.elapsed())
}

async fn streaming(transport: &RetryTransport) -> ClientError {
    transport
        .send_streaming_request("SubscribeToTask", serde_json::json!({}), &HashMap::new())
        .await
        .expect_err("the transport always fails")
}

fn calls(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}

// ── a server-directed delay longer than max_backoff ──────────────────────────

#[tokio::test(start_paused = true)]
async fn a_delay_equal_to_max_backoff_is_waited_out_in_full() {
    let (inner, count) = Failing::new(exactly_max_backoff);
    let transport = RetryTransport::new(Box::new(inner), policy(1));
    let (error, took) = unary(&transport).await;
    assert_eq!(calls(&count), 2, "one retry");
    assert_eq!(took, MAX_BACKOFF, "after exactly the delay asked for");
    assert_eq!(error.retry_after(), Some(MAX_BACKOFF));
}

#[tokio::test(start_paused = true)]
async fn a_delay_longer_than_max_backoff_is_not_retried_early_and_the_caller_is_told_it() {
    let (inner, count) = Failing::new(a_second_over_max_backoff);
    let transport = RetryTransport::new(Box::new(inner), policy(3));
    let (error, took) = unary(&transport).await;
    assert_eq!(calls(&count), 1, "no retry inside the penalty window");
    assert_eq!(took, Duration::ZERO, "and no wait");
    assert_eq!(
        error.retry_after(),
        Some(MAX_BACKOFF + secs(1)),
        "the error carries the delay for the caller to act on"
    );
}

#[tokio::test(start_paused = true)]
async fn a_streaming_start_follows_the_same_rule() {
    let (inner, count) = Failing::new(a_second_over_max_backoff);
    let transport = RetryTransport::new(Box::new(inner), policy(3));
    let error = streaming(&transport).await;
    assert_eq!(calls(&count), 1);
    assert_eq!(error.retry_after(), Some(MAX_BACKOFF + secs(1)));

    let (inner, count) = Failing::new(exactly_max_backoff);
    let transport = RetryTransport::new(Box::new(inner), policy(1));
    let started = Instant::now();
    streaming(&transport).await;
    assert_eq!(calls(&count), 2);
    assert_eq!(started.elapsed(), MAX_BACKOFF);
}

// ── max_elapsed ──────────────────────────────────────────────────────────────

#[tokio::test(start_paused = true)]
async fn a_retry_that_would_start_exactly_at_max_elapsed_is_made_and_one_after_is_not() {
    // Two retries of 30 s each: the first starts at 30 s, the second at 60 s.
    let run = |max: Duration| async move {
        let (inner, count) = Failing::new(exactly_max_backoff);
        let transport = RetryTransport::new(Box::new(inner), policy(2).with_max_elapsed(max));
        let (_, took) = unary(&transport).await;
        (calls(&count), took)
    };
    assert_eq!(run(secs(60)).await, (3, secs(60)), "both retries fit");
    assert_eq!(
        run(just_under(secs(60))).await,
        (2, secs(30)),
        "the second starts a nanosecond too late"
    );
    assert_eq!(
        run(secs(30)).await,
        (2, secs(30)),
        "the first fits exactly, the second does not"
    );
    assert_eq!(
        run(just_under(secs(30))).await,
        (1, Duration::ZERO),
        "the first starts a nanosecond too late"
    );
}

#[tokio::test(start_paused = true)]
async fn without_max_elapsed_nothing_but_max_retries_stops_a_call() {
    let (inner, count) = Failing::new(exactly_max_backoff);
    let transport = RetryTransport::new(Box::new(inner), policy(3));
    let (_, took) = unary(&transport).await;
    assert_eq!((calls(&count), took), (4, secs(90)));
}

#[tokio::test(start_paused = true)]
async fn max_elapsed_stops_a_streaming_start_too() {
    let (inner, count) = Failing::new(exactly_max_backoff);
    let transport = RetryTransport::new(
        Box::new(inner),
        policy(3).with_max_elapsed(just_under(secs(30))),
    );
    streaming(&transport).await;
    assert_eq!(calls(&count), 1);
}

// ── the budget ───────────────────────────────────────────────────────────────

#[tokio::test(start_paused = true)]
async fn a_retry_the_budget_cannot_pay_for_is_not_made() {
    let budget = RetryBudget::new(0.0, 0.0).with_reserve(2.0);
    let (inner, count) = Failing::new(timeout);
    let transport = RetryTransport::new(Box::new(inner), policy(5).with_budget(budget));

    let (error, _) = unary(&transport).await;
    assert_eq!(
        calls(&count),
        3,
        "the first attempt and the two retries paid for"
    );
    assert!(matches!(error, ClientError::Timeout(_)), "the last error");

    unary(&transport).await;
    assert_eq!(calls(&count), 4, "the next call is one attempt");
}

#[tokio::test(start_paused = true)]
async fn each_call_earns_its_fraction_of_a_retry() {
    // A reserve of one and half a retry per call: the sequence of attempts is
    // 2 (spends the reserve), 1 (half), 2 (a whole retry again).
    let budget = RetryBudget::new(0.5, 0.0).with_reserve(1.0);
    let (inner, count) = Failing::new(timeout);
    let transport = RetryTransport::new(Box::new(inner), policy(5).with_budget(budget));
    let mut seen = Vec::new();
    for _ in 0..3 {
        let before = calls(&count);
        unary(&transport).await;
        seen.push(calls(&count) - before);
    }
    assert_eq!(seen, [2, 1, 2]);
}

#[tokio::test(start_paused = true)]
async fn a_streaming_start_spends_and_earns_from_the_same_budget() {
    let budget = RetryBudget::new(0.5, 0.0).with_reserve(1.0);
    let (inner, count) = Failing::new(timeout);
    let transport = RetryTransport::new(Box::new(inner), policy(5).with_budget(budget));
    let mut seen = Vec::new();
    for _ in 0..3 {
        let before = calls(&count);
        streaming(&transport).await;
        seen.push(calls(&count) - before);
    }
    assert_eq!(seen, [2, 1, 2]);
}

#[tokio::test(start_paused = true)]
async fn a_retry_that_another_limit_stopped_costs_the_budget_nothing() {
    // One retry in the reserve. A call told to wait too long gives up without
    // spending it, and the next call still has it.
    let budget = RetryBudget::new(0.0, 0.0).with_reserve(1.0);
    let (inner, count) = Failing::new(a_second_over_max_backoff);
    let transport = RetryTransport::new(Box::new(inner), policy(3).with_budget(budget));
    unary(&transport).await;
    assert_eq!(calls(&count), 1);

    let (inner, count) = Failing::new(exactly_max_backoff);
    let transport = RetryTransport::new(
        Box::new(inner),
        policy(3)
            .with_budget(budget)
            .with_max_elapsed(just_under(secs(30))),
    );
    unary(&transport).await;
    assert_eq!(calls(&count), 1, "stopped by max_elapsed");
    let (inner, count) = Failing::new(timeout);
    let transport = RetryTransport::new(Box::new(inner), policy(3).with_budget(budget));
    unary(&transport).await;
    assert_eq!(
        calls(&count),
        2,
        "and a fresh client still has its one retry"
    );
}

#[test]
fn the_new_policy_fields_default_to_no_limit_and_their_setters_set_them() {
    let default = RetryPolicy::default();
    assert_eq!((default.budget, default.max_elapsed), (None, None));
    let set = default
        .with_budget(RetryBudget::new(0.2, 3.0))
        .with_max_elapsed(secs(7));
    assert_eq!(set.budget, Some(RetryBudget::new(0.2, 3.0)));
    assert_eq!(set.max_elapsed, Some(secs(7)));
}
