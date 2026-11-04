// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A webhook's own answers decide what becomes of its config (audit D-05),
//! with the real sender against real sockets and the delivery path that reads
//! the config list afresh for each event.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::metrics::Metrics;
use crate::push::health::watched;
use crate::push::{HttpPushSender, PushRetryPolicy, PushSender};

/// A webhook that answers every request with `status_line` and counts them.
async fn webhook(status_line: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = stream.read(&mut buf).await;
                seen.fetch_add(1, Ordering::SeqCst);
                let reply = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            });
        }
    });
    (url, count)
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

async fn with_config(url: &str) -> Arc<InMemoryPushConfigStore> {
    let store = Arc::new(InMemoryPushConfigStore::new());
    let mut config = TaskPushNotificationConfig::new("t1", url);
    config.id = Some("c1".to_owned());
    store.set(config).await.unwrap();
    store
}

fn sender_for(store: &Arc<InMemoryPushConfigStore>, limits: &HandlerLimits) -> Arc<dyn PushSender> {
    let http = HttpPushSender::new()
        .allow_private_urls()
        .with_retry_policy(PushRetryPolicy::default().with_max_attempts(1));
    watched(
        Arc::new(http),
        Arc::clone(store) as Arc<dyn PushConfigStore>,
        limits,
    )
}

async fn deliver(
    store: &InMemoryPushConfigStore,
    sender: &dyn PushSender,
    limits: &HandlerLimits,
    metrics: &Recorder,
    state: TaskState,
) {
    deliver_push_bg(
        &TaskId::new("t1"),
        &make_status_event("t1", state),
        store,
        Some(sender),
        limits,
        metrics,
    )
    .await;
}

#[tokio::test]
async fn a_410_removes_the_config_and_the_next_event_reaches_nobody() {
    let (url, requests) = webhook("410 Gone").await;
    let store = with_config(&url).await;
    let limits = HandlerLimits::default();
    let sender = sender_for(&store, &limits);
    let metrics = Recorder::default();

    deliver(&store, &*sender, &limits, &metrics, TaskState::Working).await;
    assert!(
        store.list("t1").await.unwrap().is_empty(),
        "the webhook said it is gone, and its config is"
    );
    deliver(&store, &*sender, &limits, &metrics, TaskState::Working).await;

    assert_eq!(requests.load(Ordering::SeqCst), 1, "no second request");
    assert_eq!(metrics.outcomes(), ["gone"]);
}

#[tokio::test]
async fn a_410_is_only_a_failure_when_removal_is_off() {
    let (url, requests) = webhook("410 Gone").await;
    let store = with_config(&url).await;
    let limits = HandlerLimits::default().with_push_remove_gone(false);
    let sender = sender_for(&store, &limits);
    let metrics = Recorder::default();

    for _ in 0..2 {
        deliver(&store, &*sender, &limits, &metrics, TaskState::Working).await;
    }
    assert_eq!(
        store.list("t1").await.unwrap().len(),
        1,
        "the config is kept"
    );
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(metrics.outcomes(), ["failed", "failed"]);
}

#[tokio::test]
async fn failing_webhooks_are_suspended_and_an_ending_event_still_reaches_them() {
    let (url, requests) = webhook("500 Internal Server Error").await;
    let store = with_config(&url).await;
    let limits = HandlerLimits::default().with_push_suspend_after(2);
    let sender = sender_for(&store, &limits);
    let metrics = Recorder::default();

    for _ in 0..5 {
        deliver(&store, &*sender, &limits, &metrics, TaskState::Working).await;
    }
    deliver(&store, &*sender, &limits, &metrics, TaskState::Completed).await;

    assert_eq!(
        metrics.outcomes(),
        [
            "failed",
            "failed",
            "suspended",
            "suspended",
            "suspended",
            "failed"
        ],
        "two failures suspend it, three events are skipped, and the one that ends the task is tried"
    );
    assert_eq!(requests.load(Ordering::SeqCst), 3);
    assert_eq!(
        store.list("t1").await.unwrap().len(),
        1,
        "a suspended config is kept"
    );
}

#[tokio::test]
async fn suspension_off_tries_every_event() {
    let (url, requests) = webhook("500 Internal Server Error").await;
    let store = with_config(&url).await;
    let limits = HandlerLimits::default().with_push_suspend_after(0);
    let sender = sender_for(&store, &limits);
    let metrics = Recorder::default();

    for _ in 0..8 {
        deliver(&store, &*sender, &limits, &metrics, TaskState::Working).await;
    }
    assert_eq!(requests.load(Ordering::SeqCst), 8);
    assert!(metrics.outcomes().iter().all(|o| o == "failed"));
}

/// A blocking send hands over the events of its request together, with the
/// configs listed once: the event after the one that learned of the `410`
/// still reaches the webhook, which answers `410` again. The next batch lists
/// afresh and finds nothing. (The book's outcomes table says so.)
#[tokio::test]
async fn a_batch_that_listed_the_config_once_reaches_a_gone_webhook_again() {
    let (url, requests) = webhook("410 Gone").await;
    let store = with_config(&url).await;
    let limits = HandlerLimits::default();
    let sender = sender_for(&store, &limits);
    let metrics = Recorder::default();
    let configs = store.list("t1").await.unwrap();

    for _ in 0..2 {
        deliver_to_every_config(
            &TaskId::new("t1"),
            &*sender,
            &configs,
            &make_status_event("t1", TaskState::Working),
            limits.push_delivery_timeout,
            1,
            &metrics,
            push_outcome::TIMEOUT,
        )
        .await;
    }
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(metrics.outcomes(), ["gone", "gone"]);
    assert!(store.list("t1").await.unwrap().is_empty());
}
