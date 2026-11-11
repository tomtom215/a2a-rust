// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A worker's claim on a row is a short lease it renews while it works (ADR 0037,
//! audit H14): the rows of a worker that died are free after one short lease, not
//! after the longest a delivery can take, and the rows of one that is alive are
//! not taken from it however long a delivery runs.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

/// Counts its deliveries, each taking `delay` (or never ending, for `None`).
struct Slow {
    calls: AtomicUsize,
    delay: Option<Duration>,
}

impl Slow {
    fn new(delay: Option<Duration>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            delay,
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl PushSender for Slow {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match self.delay {
                Some(delay) => tokio::time::sleep(delay).await,
                None => std::future::pending::<()>().await,
            }
            Ok(())
        })
    }
}

/// The in-memory outbox whose renewals can be made to stop, as they do when a
/// worker dies; one that does not renew at all; both for the claims below.
struct Heartbeat {
    inner: Arc<InMemoryPushOutbox>,
    beats: AtomicBool,
}

impl PushOutbox for Heartbeat {
    fn record(&self, d: Vec<NewDelivery>, cap: usize) -> OutboxFuture<'_, usize> {
        self.inner.record(d, cap)
    }
    fn claim<'a>(
        &'a self,
        owner: &'a str,
        max: usize,
        lease: Duration,
    ) -> OutboxFuture<'a, Vec<crate::push::outbox::ClaimedDelivery>> {
        self.inner.claim(owner, max, lease)
    }
    fn complete(&self, seq: i64) -> OutboxFuture<'_, ()> {
        self.inner.complete(seq)
    }
    fn retry_after<'a>(
        &'a self,
        seq: i64,
        owner: &'a str,
        after: Duration,
    ) -> OutboxFuture<'a, ()> {
        self.inner.retry_after(seq, owner, after)
    }
    fn pending(&self) -> OutboxFuture<'_, usize> {
        self.inner.pending()
    }
    fn supports_renewal(&self) -> bool {
        true
    }
    fn renew<'a>(&'a self, owner: &'a str, lease: Duration) -> OutboxFuture<'a, usize> {
        if self.beats.load(Ordering::SeqCst) {
            self.inner.renew(owner, lease)
        } else {
            Box::pin(async { Ok(0) })
        }
    }
}

fn lanes_over(s: &Setup, outbox: Arc<dyn PushOutbox>, sender: Arc<dyn PushSender>) -> PushLanes {
    PushLanes::with_outbox(
        Arc::clone(&s.store) as Arc<dyn PushConfigStore>,
        Some(sender),
        // A delivery attempt is bounded by this; here it is longer than any wait
        // below, so a slow delivery is slow and not retried.
        HandlerLimits::default().with_push_delivery_timeout(Duration::from_secs(10_000)),
        Arc::clone(&s.metrics) as Arc<dyn Metrics>,
        TaskTracker::new(),
        Some((outbox, CancellationToken::new())),
    )
}

fn completed(lanes: &PushLanes) {
    lanes.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Completed),
    );
}

/// A delivery that takes three times the lease is not taken from the worker that is
/// making it: that worker renews, and the other one finds the row leased.
#[tokio::test(start_paused = true)]
async fn a_worker_that_is_alive_keeps_its_row_past_the_lease() {
    let s = setup(&["http://a.example.com/h"]).await;
    let outbox = Arc::new(Heartbeat {
        inner: Arc::clone(&s.outbox),
        beats: AtomicBool::new(true),
    });
    let (slow, other) = (Slow::new(Some(Duration::from_secs(100))), Slow::new(None));
    let first = lanes_over(
        &s,
        Arc::clone(&outbox) as Arc<dyn PushOutbox>,
        Arc::clone(&slow) as _,
    );
    completed(&first);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(slow.calls(), 1, "the first worker has the row");
    let _second = lanes_over(&s, outbox as Arc<dyn PushOutbox>, Arc::clone(&other) as _);

    tokio::time::sleep(Duration::from_secs(290)).await;
    assert_eq!(
        other.calls(),
        0,
        "the row is not taken from a worker that renews"
    );
    drained(&s.outbox).await;
    assert_eq!(slow.calls(), 1);
}

/// A worker that stops renewing (it died) loses its row after the short lease
/// and another delivers it: well before the longest a delivery can take (the
/// lease for an outbox that does not renew is 120 s).
#[tokio::test(start_paused = true)]
async fn a_worker_that_stops_renewing_loses_its_row_after_the_short_lease() {
    let s = setup(&["http://a.example.com/h"]).await;
    let outbox = Arc::new(Heartbeat {
        inner: Arc::clone(&s.outbox),
        beats: AtomicBool::new(true),
    });
    let (dead, alive) = (Slow::new(None), Slow::new(Some(Duration::ZERO)));
    let first = lanes_over(
        &s,
        Arc::clone(&outbox) as Arc<dyn PushOutbox>,
        Arc::clone(&dead) as _,
    );
    completed(&first);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(dead.calls(), 1);
    // It dies: no more renewals. The second worker starts after.
    outbox.beats.store(false, Ordering::SeqCst);
    let _second = lanes_over(&s, outbox as Arc<dyn PushOutbox>, Arc::clone(&alive) as _);

    let started = tokio::time::Instant::now();
    tokio::time::sleep(Duration::from_secs(25)).await;
    assert_eq!(
        alive.calls(),
        0,
        "inside the lease, the row is still the first worker's"
    );
    while alive.calls() == 0 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the row stayed with a dead worker"
        );
    }
    assert!(
        started.elapsed() < Duration::from_secs(45),
        "{:?}",
        started.elapsed()
    );
}

/// The lease a worker asks for is the short one when the outbox renews and the
/// long one when it does not.
#[test]
fn the_lease_is_short_only_when_the_outbox_renews() {
    struct NoRenewal(InMemoryPushOutbox);
    impl PushOutbox for NoRenewal {
        fn record(&self, d: Vec<NewDelivery>, cap: usize) -> OutboxFuture<'_, usize> {
            self.0.record(d, cap)
        }
        fn claim<'a>(
            &'a self,
            owner: &'a str,
            max: usize,
            lease: Duration,
        ) -> OutboxFuture<'a, Vec<crate::push::outbox::ClaimedDelivery>> {
            self.0.claim(owner, max, lease)
        }
        fn complete(&self, seq: i64) -> OutboxFuture<'_, ()> {
            self.0.complete(seq)
        }
        fn retry_after<'a>(
            &'a self,
            seq: i64,
            owner: &'a str,
            after: Duration,
        ) -> OutboxFuture<'a, ()> {
            self.0.retry_after(seq, owner, after)
        }
        fn pending(&self) -> OutboxFuture<'_, usize> {
            self.0.pending()
        }
    }
    let timeout = Duration::from_secs(30);
    assert_eq!(
        outbox_worker::claim_lease(&InMemoryPushOutbox::new(), timeout),
        outbox_worker::RENEWED_LEASE
    );
    assert_eq!(
        outbox_worker::claim_lease(&NoRenewal(InMemoryPushOutbox::new()), timeout),
        outbox_worker::lease(timeout)
    );
    assert!(outbox_worker::RENEWED_LEASE >= 3 * outbox_worker::RENEW_EVERY);
}
