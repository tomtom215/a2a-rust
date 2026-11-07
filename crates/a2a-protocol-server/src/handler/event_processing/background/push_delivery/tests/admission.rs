// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A delivery's wait for a slot in the push gate does not spend the time its
//! webhook is given (audit NEW-f-2), through the handler's own chain: the
//! health watch outside the gate, the gate, then the sender.
//!
//! The clock is paused, so every duration below is exact.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::metrics::{Metrics, NoopMetrics};
use crate::push::PushSender;
use crate::push::gate::PushGate;
use crate::push::health::watched;

/// Answers each URL after the delay its path names (`/8s`), or never
/// (`/never`).
struct Sleeper;

impl PushSender for Sleeper {
    fn send<'a>(
        &'a self,
        url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // `/at-one/` in the path: the name resolves to one shared
            // address, held as `HttpPushSender` holds what it resolved.
            let _hold = if url.contains("/at-one/") {
                crate::push::gate::admission::hold_addresses(&["203.0.113.9".parse().unwrap()])
                    .await
            } else {
                None
            };
            let path = url.rsplit('/').next().unwrap_or_default();
            match path.strip_suffix('s').and_then(|n| n.parse::<u64>().ok()) {
                Some(secs) => tokio::time::sleep(Duration::from_secs(secs)).await,
                None => std::future::pending::<()>().await,
            }
            Ok(())
        })
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);

impl Metrics for Recorder {
    fn on_push_delivery(&self, outcome: &str) {
        self.0.lock().unwrap().push(outcome.to_owned());
    }
}

impl Recorder {
    fn sorted(&self) -> Vec<String> {
        let mut outcomes = self.0.lock().unwrap().clone();
        outcomes.sort();
        outcomes
    }
}

fn configs(urls: &[&str]) -> Vec<TaskPushNotificationConfig> {
    urls.iter()
        .enumerate()
        .map(|(i, url)| TaskPushNotificationConfig::new("t1", *url).with_id(format!("c{i}")))
        .collect()
}

/// The handler's chain around [`Sleeper`], one delivery at a time overall.
fn chain(limits: &HandlerLimits) -> Arc<dyn PushSender> {
    let gate = PushGate::new(Arc::new(Sleeper), limits, Arc::new(NoopMetrics));
    watched(
        Arc::new(gate),
        Arc::new(InMemoryPushConfigStore::new()) as Arc<dyn PushConfigStore>,
        limits,
    )
}

async fn deliver(sender: &dyn PushSender, urls: &[&str], timeout: Duration, metrics: &Recorder) {
    deliver_to_every_config(
        &TaskId::new("t1"),
        sender,
        &configs(urls),
        &make_status_event("t1", TaskState::Working),
        timeout,
        urls.len(),
        metrics,
        push_outcome::TIMEOUT,
    )
    .await;
}

/// The second delivery waits 8 s for the only slot, then its webhook takes
/// 8 s: 16 s in all against a 10 s timeout. It is delivered, because its
/// 10 s start when it is let through. Until 0.16.0 it ended as `timeout` at
/// 10 s, two seconds into a request that would have succeeded.
#[tokio::test(start_paused = true)]
async fn a_delivery_let_through_late_has_the_whole_timeout_for_its_webhook() {
    let limits = HandlerLimits::default().with_push_max_in_flight(1);
    let sender = chain(&limits);
    let metrics = Recorder::default();
    let started = tokio::time::Instant::now();

    deliver(
        &*sender,
        &["http://a.example.com/8s", "http://b.example.com/8s"],
        Duration::from_secs(10),
        &metrics,
    )
    .await;

    assert_eq!(metrics.sorted(), ["delivered", "delivered"]);
    assert_eq!(started.elapsed(), Duration::from_secs(16));
}

/// A delivery that waits the whole timeout and is never let through made no
/// request: it is `throttled`, not `timeout`, and the health watch does not
/// count it against its webhook, which takes the next event at once. The
/// only slot is held by a delivery made outside the handler, which has no
/// clock, until the test ends it.
#[tokio::test(start_paused = true)]
async fn a_delivery_never_let_through_is_throttled_and_not_held_against_its_webhook() {
    let limits = HandlerLimits::default()
        .with_push_max_in_flight(1)
        .with_push_suspend_after(1);
    let sender = chain(&limits);
    let hog = {
        let sender = Arc::clone(&sender);
        tokio::spawn(async move {
            let url = "http://hog.example.com/never";
            let config = TaskPushNotificationConfig::new("t1", url).with_id("hog".to_owned());
            let event = make_status_event("t1", TaskState::Working);
            let _ = sender.send(url, &event, &config).await;
        })
    };
    tokio::task::yield_now().await;
    let metrics = Recorder::default();
    let started = tokio::time::Instant::now();

    let b = ["http://b.example.com/0s"];
    deliver(&*sender, &b, Duration::from_secs(10), &metrics).await;
    assert_eq!(metrics.sorted(), ["throttled"]);
    assert_eq!(started.elapsed(), Duration::from_secs(10));

    hog.abort();
    let _ = hog.await;
    let metrics = Recorder::default();
    deliver(&*sender, &b, Duration::from_secs(10), &metrics).await;
    assert_eq!(
        metrics.sorted(),
        ["delivered"],
        "a throttled delivery suspended its webhook"
    );
}

/// Two names, one address, a bound of one per destination: the second
/// delivery is let through by name, then waits 8 s for the address, then its
/// webhook takes 8 s. Its clock starts again when the address frees, so it is
/// delivered at 16 s against a 10 s timeout, as a wait for any other slot is.
#[tokio::test(start_paused = true)]
async fn a_wait_for_an_address_does_not_spend_the_webhooks_time_either() {
    let limits = HandlerLimits::default().with_push_max_in_flight_per_host(1);
    let sender = chain(&limits);
    let metrics = Recorder::default();
    let started = tokio::time::Instant::now();

    deliver(
        &*sender,
        &[
            "http://a.example.com/at-one/8s",
            "http://b.example.com/at-one/8s",
        ],
        Duration::from_secs(10),
        &metrics,
    )
    .await;

    assert_eq!(metrics.sorted(), ["delivered", "delivered"]);
    assert_eq!(started.elapsed(), Duration::from_secs(16));
}

/// A delivery that waited, was let through, and whose webhook then did not
/// answer is the webhook's `timeout`, not `throttled`: it made its request.
/// The only slot is held for 5 s by a delivery made first, outside the
/// handler; the delivery waits those 5 s, then has its whole 10 s.
#[tokio::test(start_paused = true)]
async fn a_delivery_that_waited_and_then_timed_out_is_a_timeout() {
    let limits = HandlerLimits::default().with_push_max_in_flight(1);
    let sender = chain(&limits);
    let first = {
        let sender = Arc::clone(&sender);
        tokio::spawn(async move {
            let url = "http://first.example.com/5s";
            let config = TaskPushNotificationConfig::new("t1", url).with_id("first".to_owned());
            let event = make_status_event("t1", TaskState::Working);
            sender.send(url, &event, &config).await.unwrap();
        })
    };
    tokio::task::yield_now().await;
    let metrics = Recorder::default();
    let started = tokio::time::Instant::now();

    deliver(
        &*sender,
        &["http://b.example.com/never"],
        Duration::from_secs(10),
        &metrics,
    )
    .await;

    assert_eq!(metrics.sorted(), ["timeout"]);
    assert_eq!(started.elapsed(), Duration::from_secs(15));
    first.await.unwrap();
}
