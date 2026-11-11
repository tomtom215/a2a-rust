// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The push lanes with an outbox (ADR 0037): what is recorded, what the
//! worker delivers, retries, ends, and leaves for the next process.
//!
//! The clock is paused: a backoff or a poll passes as soon as nothing else
//! can run, so these tests wait for what they assert, never for time.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use a2a_protocol_types::error::A2aResult;

use super::*;
use crate::metrics::Metrics;
use crate::push::outbox::{InMemoryPushOutbox, NewDelivery, OutboxFuture, PushOutbox};
use crate::push::{PushSender, current_delivery_id};

mod faults;
mod lease;

/// One delivery as the sender saw it.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    url: String,
    delivery_id: Option<String>,
    state: TaskState,
}

/// A sender that answers each URL from a script (then `Ok`), and reports
/// each delivery on a channel.
struct Scripted {
    answers: Mutex<std::collections::HashMap<String, VecDeque<A2aResult<()>>>>,
    seen: mpsc::UnboundedSender<Seen>,
}

impl Scripted {
    fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<Seen>) {
        let (seen, rx) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                answers: Mutex::default(),
                seen,
            }),
            rx,
        )
    }

    fn answer(&self, url: &str, answers: Vec<A2aResult<()>>) {
        self.answers
            .lock()
            .unwrap()
            .insert(url.to_owned(), answers.into());
    }
}

impl PushSender for Scripted {
    fn send<'a>(
        &'a self,
        url: &'a str,
        event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let state = match event {
                StreamResponse::StatusUpdate(u) => u.status.state,
                _ => TaskState::Unspecified,
            };
            let _ = self.seen.send(Seen {
                url: url.to_owned(),
                delivery_id: current_delivery_id(),
                state,
            });
            self.answers
                .lock()
                .unwrap()
                .get_mut(url)
                .and_then(VecDeque::pop_front)
                .unwrap_or(Ok(()))
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
    fn outcomes(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct Setup {
    store: Arc<InMemoryPushConfigStore>,
    outbox: Arc<InMemoryPushOutbox>,
    sender: Arc<Scripted>,
    seen: mpsc::UnboundedReceiver<Seen>,
    metrics: Arc<Recorder>,
    shutdown: CancellationToken,
}

async fn setup(urls: &[&str]) -> Setup {
    let store = Arc::new(InMemoryPushConfigStore::new());
    for (i, url) in urls.iter().enumerate() {
        store
            .set(TaskPushNotificationConfig::new("t1", *url).with_id(format!("c{i}")))
            .await
            .unwrap();
    }
    let (sender, seen) = Scripted::new();
    Setup {
        store,
        outbox: Arc::new(InMemoryPushOutbox::new()),
        sender,
        seen,
        metrics: Arc::default(),
        shutdown: CancellationToken::new(),
    }
}

fn lanes(s: &Setup, limits: HandlerLimits) -> PushLanes {
    PushLanes::with_outbox(
        Arc::clone(&s.store) as Arc<dyn PushConfigStore>,
        Some(Arc::clone(&s.sender) as Arc<dyn PushSender>),
        limits,
        Arc::clone(&s.metrics) as Arc<dyn Metrics>,
        TaskTracker::new(),
        Some((
            Arc::clone(&s.outbox) as Arc<dyn PushOutbox>,
            s.shutdown.clone(),
        )),
    )
}

/// Waits until the outbox is empty.
async fn drained(outbox: &InMemoryPushOutbox) {
    while outbox.pending().await.unwrap() > 0 {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn rejected(status: u16) -> A2aResult<()> {
    Err(crate::push::health::rejected(
        format!("HTTP {status}"),
        status,
    ))
}

fn failed() -> A2aResult<()> {
    Err(A2aError::internal("connection refused"))
}

/// A webhook that fails twice is delivered on the third attempt, each with
/// the row's delivery id, and the row is then gone.
#[tokio::test(start_paused = true)]
async fn a_failing_webhook_is_retried_with_one_delivery_id_until_it_answers() {
    let mut s = setup(&["http://a.example.com/h"]).await;
    s.sender
        .answer("http://a.example.com/h", vec![failed(), failed()]);
    let lanes = lanes(&s, HandlerLimits::default());

    lanes.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Completed),
    );
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(s.seen.recv().await.unwrap().delivery_id.expect("an id"));
    }
    drained(&s.outbox).await;
    assert!(ids.iter().all(|id| *id == ids[0]), "{ids:?}");
    assert_eq!(s.metrics.outcomes(), ["failed", "failed", "delivered"]);
}

/// A failed delivery comes due again when its backoff has passed (a second at
/// most, then a poll), not when the row's claim runs out (two minutes): the
/// worker hands the row back with `retry_after`. Without that it is claimed
/// again only once its lease ends, and still delivered, which is why the retry
/// test above cannot tell the two apart.
#[tokio::test(start_paused = true)]
async fn a_failed_delivery_is_retried_after_its_backoff_not_its_lease() {
    let mut s = setup(&["http://a.example.com/h"]).await;
    s.sender.answer("http://a.example.com/h", vec![failed()]);
    let limits = HandlerLimits::default();
    let lease = outbox_worker::lease(limits.push_delivery_timeout);
    let lanes = lanes(&s, limits);

    lanes.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Completed),
    );
    s.seen.recv().await.unwrap();
    let failed_at = tokio::time::Instant::now();
    s.seen.recv().await.unwrap();
    let waited = failed_at.elapsed();
    assert!(
        waited < Duration::from_secs(5),
        "retried after {waited:?}; the row's lease is {lease:?}"
    );
    assert!(
        lease > 10 * waited,
        "{waited:?} against a lease of {lease:?}"
    );
}

/// A refusal no retry will change ends the row after one attempt.
#[tokio::test(start_paused = true)]
async fn a_refusal_for_good_ends_the_row() {
    let mut s = setup(&["http://a.example.com/h"]).await;
    s.sender
        .answer("http://a.example.com/h", vec![rejected(400)]);
    let lanes = lanes(&s, HandlerLimits::default());

    lanes.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Working),
    );
    s.seen.recv().await.unwrap();
    drained(&s.outbox).await;
    assert_eq!(s.metrics.outcomes(), ["failed"]);
}

/// A webhook's events go out in the order they were stored: the second
/// waits while the first is retried.
#[tokio::test(start_paused = true)]
async fn a_webhooks_events_go_out_in_order_across_retries() {
    let mut s = setup(&["http://a.example.com/h"]).await;
    s.sender.answer("http://a.example.com/h", vec![failed()]);
    let lanes = lanes(&s, HandlerLimits::default());

    lanes.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Working),
    );
    lanes.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Completed),
    );
    let mut states = Vec::new();
    for _ in 0..3 {
        states.push(s.seen.recv().await.unwrap().state);
    }
    assert_eq!(
        states,
        [TaskState::Working, TaskState::Working, TaskState::Completed]
    );
}

/// A config deleted before its row is delivered ends the row, unsent.
#[tokio::test(start_paused = true)]
async fn a_row_whose_config_is_gone_ends_unsent() {
    let s = setup(&["http://a.example.com/h"]).await;
    s.outbox
        .record(
            vec![NewDelivery {
                delivery_id: "d1".to_owned(),
                tenant: String::new(),
                caller: None,
                task_id: "t1".to_owned(),
                config_id: "deleted".to_owned(),
                event: make_status_event("t1", TaskState::Working),
                ends_turn: false,
            }],
            10,
        )
        .await
        .unwrap();
    let _lanes = lanes(&s, HandlerLimits::default());
    drained(&s.outbox).await;
    assert!(
        s.metrics.outcomes().is_empty(),
        "nothing sent, nothing counted"
    );
}

/// A row older than `push_outbox_max_age` is dropped and counted `expired`.
#[tokio::test(start_paused = true)]
async fn a_row_past_its_age_expires_unsent() {
    let mut s = setup(&["http://a.example.com/h"]).await;
    s.sender.answer("http://a.example.com/h", vec![failed()]);
    let limits = HandlerLimits::default().with_push_outbox_max_age(Duration::from_millis(500));
    let lanes = lanes(&s, limits);

    lanes.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Working),
    );
    s.seen.recv().await.unwrap();
    drained(&s.outbox).await;
    assert_eq!(s.metrics.outcomes(), ["failed", "expired"]);
    assert!(s.seen.try_recv().is_err(), "no second attempt");
}

/// What one handler's lanes recorded and did not deliver, the next one
/// delivers, with the same delivery id: a restart, with the outbox the
/// store both share.
#[tokio::test(start_paused = true)]
async fn what_one_handler_left_the_next_one_delivers() {
    let mut s = setup(&["http://a.example.com/h"]).await;
    s.sender.answer("http://a.example.com/h", vec![failed()]);
    let first = lanes(&s, HandlerLimits::default());
    first.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Completed),
    );
    let tried = s.seen.recv().await.unwrap();

    // The first handler stops.
    s.shutdown.cancel();
    drop(first);
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(
        s.outbox.pending().await.unwrap(),
        1,
        "left for the next process"
    );

    s.shutdown = CancellationToken::new();
    let _second = lanes(&s, HandlerLimits::default());
    let delivered = s.seen.recv().await.unwrap();
    drained(&s.outbox).await;
    assert_eq!(delivered.delivery_id, tried.delivery_id);
}

/// At shutdown a row being delivered is released at once, not after its
/// lease: the next claim takes it, one attempt on.
#[tokio::test(start_paused = true)]
async fn shutdown_releases_a_row_mid_delivery() {
    struct Hangs(mpsc::UnboundedSender<()>);
    impl PushSender for Hangs {
        fn send<'a>(
            &'a self,
            _url: &'a str,
            _event: &'a StreamResponse,
            _config: &'a TaskPushNotificationConfig,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            let _ = self.0.send(());
            Box::pin(std::future::pending())
        }
    }
    let s = setup(&["http://a.example.com/h"]).await;
    let (started, mut sending) = mpsc::unbounded_channel();
    let lanes = PushLanes::with_outbox(
        Arc::clone(&s.store) as Arc<dyn PushConfigStore>,
        Some(Arc::new(Hangs(started)) as Arc<dyn PushSender>),
        HandlerLimits::default(),
        Arc::clone(&s.metrics) as Arc<dyn Metrics>,
        TaskTracker::new(),
        Some((
            Arc::clone(&s.outbox) as Arc<dyn PushOutbox>,
            s.shutdown.clone(),
        )),
    );
    lanes.push(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Working),
    );
    sending.recv().await.unwrap();
    s.shutdown.cancel();
    // The lease is 120 s; the row is free within one.
    assert_eq!(
        outbox_worker::lease(HandlerLimits::default().push_delivery_timeout),
        Duration::from_secs(120)
    );
    let claimed = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let claimed = s
                .outbox
                .claim("probe", 10, Duration::from_secs(60))
                .await
                .unwrap();
            if !claimed.is_empty() {
                return claimed;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("released at shutdown, not after its lease");
    assert_eq!(claimed[0].attempts, 1);
}

/// The blocking path's batch is recorded too, every event to every config.
#[tokio::test(start_paused = true)]
async fn a_batch_is_recorded_event_by_event_and_config_by_config() {
    let mut s = setup(&["http://a.example.com/h", "http://b.example.com/h"]).await;
    let lanes = lanes(&s, HandlerLimits::default());
    lanes.enqueue_events(
        &TaskId::new("t1"),
        true,
        vec![
            make_status_event("t1", TaskState::Working),
            make_status_event("t1", TaskState::Completed),
        ],
        async { panic!("the outbox takes the batch; nothing is delivered from memory") },
    );
    let mut seen = Vec::new();
    for _ in 0..4 {
        let d = s.seen.recv().await.unwrap();
        seen.push((d.url, d.state));
    }
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        seen,
        [
            ("http://a.example.com/h".to_owned(), TaskState::Working),
            ("http://a.example.com/h".to_owned(), TaskState::Completed),
            ("http://b.example.com/h".to_owned(), TaskState::Working),
            ("http://b.example.com/h".to_owned(), TaskState::Completed),
        ]
    );
}
