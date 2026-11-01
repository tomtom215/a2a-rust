// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The gate's bounds, one at a time.
//!
//! The clock is paused, so a sender that "takes 50 ms" costs no real time and
//! the order in which waiting deliveries are served is the semaphore's FIFO
//! order, not the scheduler's mood.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
use tokio::sync::Semaphore;

use super::*;
use crate::metrics::NoopMetrics;

fn event() -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("t1"),
        context_id: ContextId::new("c1"),
        status: TaskStatus::with_timestamp(TaskState::Working),
        metadata: None,
    })
}

fn config(url: &str) -> TaskPushNotificationConfig {
    TaskPushNotificationConfig {
        tenant: None,
        id: Some("cfg".to_owned()),
        task_id: Some("t1".to_owned()),
        url: url.to_owned(),
        token: None,
        authentication: None,
    }
}

/// Counts how many sends run at once and holds each for `hold`, or, for a
/// URL in `blocked`, until [`Probe::release`].
#[derive(Default)]
struct Probe {
    now: AtomicUsize,
    max: AtomicUsize,
    done: AtomicUsize,
    hold: Duration,
    blocked: Mutex<std::collections::HashMap<String, Arc<Semaphore>>>,
    block_all: bool,
}

impl Probe {
    fn holding(hold: Duration) -> Arc<Self> {
        Arc::new(Self {
            hold,
            ..Self::default()
        })
    }

    fn blocking_everything() -> Arc<Self> {
        Arc::new(Self {
            block_all: true,
            ..Self::default()
        })
    }

    fn latch(&self, url: &str) -> Arc<Semaphore> {
        Arc::clone(
            self.blocked
                .lock()
                .unwrap()
                .entry(url.to_owned())
                .or_insert_with(|| Arc::new(Semaphore::new(0))),
        )
    }

    fn release(&self, url: &str) {
        self.latch(url).add_permits(1);
    }

    fn max(&self) -> usize {
        self.max.load(Ordering::SeqCst)
    }

    fn done(&self) -> usize {
        self.done.load(Ordering::SeqCst)
    }
}

impl PushSender for Probe {
    fn send<'a>(
        &'a self,
        url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.max.fetch_max(n, Ordering::SeqCst);
            if self.block_all {
                self.latch(url).acquire_owned().await.unwrap().forget();
            } else {
                tokio::time::sleep(self.hold).await;
            }
            self.now.fetch_sub(1, Ordering::SeqCst);
            self.done.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

/// Records every `on_error`.
#[derive(Default)]
struct Errors(Mutex<Vec<(String, String)>>);

impl Metrics for Errors {
    fn on_error(&self, method: &str, error_kind: &str) {
        self.0
            .lock()
            .unwrap()
            .push((method.to_owned(), error_kind.to_owned()));
    }
}

impl Errors {
    fn throttled(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, e)| m == "PushDelivery" && e == "push_throttled")
            .count()
    }

    fn all(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

fn gate_over(
    probe: &Arc<Probe>,
    overall: usize,
    per_host: usize,
    metrics: Arc<dyn Metrics>,
) -> Arc<PushGate> {
    let limits = HandlerLimits::default()
        .with_push_max_in_flight(overall)
        .with_push_max_in_flight_per_host(per_host);
    Arc::new(PushGate::new(
        Arc::clone(probe) as Arc<dyn PushSender>,
        &limits,
        metrics,
    ))
}

/// Sends `urls` all at once through `gate` and waits for every one.
async fn send_all(gate: &Arc<PushGate>, urls: &[String]) {
    let mut tasks = Vec::new();
    for url in urls {
        let gate = Arc::clone(gate);
        let url = url.clone();
        tasks.push(tokio::spawn(async move {
            gate.send(&url, &event(), &config(&url)).await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
}

fn hosts(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| format!("http://h{i}.example.com/hook"))
        .collect()
}

fn same_host(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| format!("http://one.example.com/hook{i}"))
        .collect()
}

/// Lets every task that can make progress do so.
async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

fn map_len(gate: &PushGate) -> usize {
    gate.hosts.lock().unwrap().len()
}

#[tokio::test(start_paused = true)]
async fn at_most_the_overall_bound_run_at_once_and_all_are_delivered() {
    let probe = Probe::holding(Duration::from_millis(50));
    let gate = gate_over(&probe, 3, 100, Arc::new(NoopMetrics));
    send_all(&gate, &hosts(12)).await;
    assert_eq!(probe.max(), 3, "saturated, never beyond");
    assert_eq!(probe.done(), 12, "every delivery still happens");
}

#[tokio::test(start_paused = true)]
async fn at_most_the_per_host_bound_run_at_once_at_one_destination() {
    let probe = Probe::holding(Duration::from_millis(50));
    let gate = gate_over(&probe, 100, 2, Arc::new(NoopMetrics));
    send_all(&gate, &same_host(10)).await;
    assert_eq!(probe.max(), 2);
    assert_eq!(probe.done(), 10);
}

#[tokio::test(start_paused = true)]
async fn destinations_do_not_share_the_per_host_bound() {
    let probe = Probe::holding(Duration::from_millis(50));
    let gate = gate_over(&probe, 100, 1, Arc::new(NoopMetrics));
    send_all(&gate, &hosts(10)).await;
    assert_eq!(probe.max(), 10, "ten destinations, one each, all at once");
}

#[tokio::test(start_paused = true)]
async fn a_zero_bound_is_read_as_one() {
    let probe = Probe::holding(Duration::from_millis(50));
    let gate = gate_over(&probe, 0, 0, Arc::new(NoopMetrics));
    tokio::time::timeout(Duration::from_secs(60), send_all(&gate, &hosts(4)))
        .await
        .expect("a zero bound must not deadlock");
    assert_eq!(probe.max(), 1);
    assert_eq!(probe.done(), 4);
}

#[tokio::test(start_paused = true)]
async fn a_saturated_destination_does_not_hold_overall_permits_while_it_waits() {
    // Overall 2, one per destination. Three deliveries to A: one runs, two
    // wait for A. If a waiter held an overall permit while it waited, B would
    // find none.
    let probe = Probe::blocking_everything();
    let gate = gate_over(&probe, 2, 1, Arc::new(NoopMetrics));
    let a = |i: usize| format!("http://a.example.com/{i}");
    for i in 0..3 {
        let (gate, url) = (Arc::clone(&gate), a(i));
        tokio::spawn(async move { gate.send(&url, &event(), &config(&url)).await });
    }
    settle().await;
    let b = "http://b.example.com/hook".to_owned();
    probe.release(&b);
    let delivered = {
        let (gate, b) = (Arc::clone(&gate), b.clone());
        tokio::time::timeout(
            Duration::from_secs(1),
            tokio::spawn(async move { gate.send(&b, &event(), &config(&b)).await }),
        )
        .await
    };
    assert!(
        delivered.is_ok(),
        "B was held out by A's waiters (overall permits taken before the per-destination one)"
    );
    for i in 0..3 {
        probe.release(&a(i));
    }
    settle().await;
    assert_eq!(probe.done(), 4);
}

#[tokio::test(start_paused = true)]
async fn the_map_holds_only_destinations_with_a_delivery_running_or_waiting() {
    let probe = Probe::holding(Duration::from_millis(10));
    let gate = gate_over(&probe, 100, 100, Arc::new(NoopMetrics));
    send_all(&gate, &hosts(50)).await;
    assert_eq!(map_len(&gate), 0, "50 distinct hosts left nothing behind");
}

#[tokio::test(start_paused = true)]
async fn a_destination_stays_counted_while_another_delivery_waits_on_it() {
    // One per destination: A1 runs, A2 waits. A1 finishes and A2 takes over.
    // Then A3 arrives. If A1's exit had removed the entry, A3 would get a
    // fresh semaphore and run beside A2.
    let probe = Probe::blocking_everything();
    let gate = gate_over(&probe, 100, 1, Arc::new(NoopMetrics));
    let url = |i: usize| format!("http://one.example.com/{i}");
    for i in 0..2 {
        let (gate, u) = (Arc::clone(&gate), url(i));
        tokio::spawn(async move { gate.send(&u, &event(), &config(&u)).await });
    }
    settle().await;
    probe.release(&url(0));
    settle().await;
    assert_eq!(map_len(&gate), 1, "A2 holds the destination");
    let (g, u) = (Arc::clone(&gate), url(2));
    tokio::spawn(async move { g.send(&u, &event(), &config(&u)).await });
    settle().await;
    assert_eq!(
        probe.max(),
        1,
        "A3 ran beside A2: the entry was dropped early"
    );
    for i in 1..3 {
        probe.release(&url(i));
        settle().await;
    }
    assert_eq!(probe.done(), 3);
    assert_eq!(map_len(&gate), 0);
}

#[tokio::test(start_paused = true)]
async fn a_delivery_cancelled_while_it_waits_leaks_nothing() {
    let probe = Probe::blocking_everything();
    let gate = gate_over(&probe, 100, 1, Arc::new(NoopMetrics));
    let first = "http://one.example.com/first".to_owned();
    let second = "http://one.example.com/second".to_owned();
    let (g, u) = (Arc::clone(&gate), first.clone());
    tokio::spawn(async move { g.send(&u, &event(), &config(&u)).await });
    settle().await;
    let (g, u) = (Arc::clone(&gate), second.clone());
    let waiter = tokio::spawn(async move { g.send(&u, &event(), &config(&u)).await });
    settle().await;
    waiter.abort();
    let _ = waiter.await;
    probe.release(&first);
    settle().await;
    assert_eq!(map_len(&gate), 0, "the cancelled waiter left an entry");
    // And its permit did not vanish with it: the destination takes a new one.
    let third = "http://one.example.com/third".to_owned();
    probe.release(&third);
    tokio::time::timeout(Duration::from_secs(1), {
        let (g, u) = (Arc::clone(&gate), third.clone());
        async move { g.send(&u, &event(), &config(&u)).await }
    })
    .await
    .expect("the destination is free again")
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_delivery_that_waited_is_counted_once_and_one_that_did_not_is_not() {
    let probe = Probe::holding(Duration::from_millis(50));
    let metrics = Arc::new(Errors::default());
    let gate = gate_over(&probe, 100, 100, metrics.clone());
    send_all(&gate, &hosts(1)).await;
    assert_eq!(metrics.all(), 0, "nothing waited, nothing reported");

    let gate = gate_over(&probe, 1, 100, metrics.clone());
    send_all(&gate, &hosts(3)).await;
    assert_eq!(metrics.throttled(), 2, "two of three waited");
    assert_eq!(metrics.all(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_delivery_that_waits_twice_is_counted_once() {
    // Overall 1, one per destination. A1 holds both. B1 gets its destination
    // and waits for the overall permit (counted). A2 waits for A's destination
    // (counted); when A1 leaves, B1 takes the overall permit and A2, now
    // holding A's destination, waits again, and must not be counted again.
    let probe = Probe::blocking_everything();
    let metrics = Arc::new(Errors::default());
    let gate = gate_over(&probe, 1, 1, metrics.clone());
    let (a1, b1, a2) = (
        "http://a.example.com/1".to_owned(),
        "http://b.example.com/1".to_owned(),
        "http://a.example.com/2".to_owned(),
    );
    for u in [&a1, &b1, &a2] {
        let (g, u) = (Arc::clone(&gate), u.clone());
        tokio::spawn(async move { g.send(&u, &event(), &config(&u)).await });
        settle().await;
    }
    assert_eq!(metrics.throttled(), 2);
    probe.release(&a1);
    settle().await;
    assert_eq!(metrics.throttled(), 2, "A2's second wait was counted again");
    probe.release(&b1);
    settle().await;
    probe.release(&a2);
    settle().await;
    assert_eq!(probe.done(), 3);
}

#[test]
fn names_for_one_destination_count_as_one() {
    let same = |a: &str, b: &str| {
        let (a, b) = (destination(a), destination(b));
        assert!(a.is_some() && a == b, "{a:?} vs {b:?}");
    };
    same("http://Example.COM./x", "http://example.com:80/y");
    same("https://example.com/a", "https://example.com:443/b");
    same("http://u:p@example.com/", "http://example.com/");
    same("http://134744072/", "http://8.8.8.8/");
    same("http://0x08080808:8080/", "http://8.8.8.8:8080/");
    same("http://[2001:db8::1]/", "http://[2001:DB8:0:0:0:0:0:1]/");
}

#[test]
fn different_destinations_are_different() {
    for (a, b) in [
        ("http://example.com/", "https://example.com/"),
        ("http://example.com/", "http://example.com:8080/"),
        ("http://a.example.com/", "http://b.example.com/"),
        ("http://8.8.8.8/", "http://8.8.4.4/"),
    ] {
        assert_ne!(destination(a), destination(b), "{a} vs {b}");
    }
    assert_eq!(destination("not a url"), None);
    assert_eq!(destination("/just/a/path"), None);
}

/// A sender that answers every question the trait asks with fixed values.
struct Answers(bool);

impl PushSender for Answers {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn allows_private_urls(&self) -> bool {
        self.0
    }

    fn allows_webhook_url(&self, url: &str) -> bool {
        self.0 && url.contains("ok")
    }

    fn requires_https(&self) -> bool {
        self.0
    }

    fn max_delivery_duration(&self) -> Option<Duration> {
        self.0.then_some(Duration::from_secs(7))
    }
}

#[test]
fn the_wrapped_sender_answers_for_itself() {
    let limits = HandlerLimits::default();
    for yes in [true, false] {
        let gate = PushGate::new(Arc::new(Answers(yes)), &limits, Arc::new(NoopMetrics));
        assert_eq!(gate.allows_private_urls(), yes);
        assert_eq!(gate.allows_webhook_url("http://ok.example.com/"), yes);
        assert!(!gate.allows_webhook_url("http://nope.example.com/"));
        assert_eq!(gate.requires_https(), yes);
        assert_eq!(
            gate.max_delivery_duration(),
            yes.then_some(Duration::from_secs(7))
        );
    }
}

#[test]
fn gated_puts_the_gate_in_front_of_a_sender() {
    let limits = HandlerLimits::default();
    let metrics: Arc<dyn Metrics> = Arc::new(NoopMetrics);
    let wrapped = gated(Arc::new(Answers(true)), &limits, &metrics);
    // The answers come through the wrapper; a bare `Answers(true)` says the same.
    assert!(wrapped.allows_private_urls());
    assert_eq!(
        format!(
            "{:?}",
            PushGate::new(Arc::new(Answers(true)), &limits, metrics)
        ),
        "PushGate { per_host: 64, .. }"
    );
}
