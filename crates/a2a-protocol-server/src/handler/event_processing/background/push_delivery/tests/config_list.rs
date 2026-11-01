// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A task's webhook list that cannot be read (audit D-04).
//!
//! Until 2026-10-31 both delivery paths returned without a word when
//! `PushConfigStore::list` failed, so a database outage looked like a task
//! with no webhooks. The test that was meant to cover it passed `None` for
//! the sender, which returns before the list is read, and so never ran the
//! line it was named for.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;
use crate::metrics::{Metrics, persistence_operation, push_outcome};

/// Fails `list` `failures` times with `error`, then answers from `inner`.
struct FlakyList {
    inner: InMemoryPushConfigStore,
    failures: Mutex<usize>,
    error: fn() -> A2aError,
}

impl PushConfigStore for FlakyList {
    fn set<'a>(
        &'a self,
        cfg: TaskPushNotificationConfig,
    ) -> Pin<
        Box<
            dyn Future<Output = a2a_protocol_types::error::A2aResult<TaskPushNotificationConfig>>
                + Send
                + 'a,
        >,
    > {
        self.inner.set(cfg)
    }

    fn get<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = a2a_protocol_types::error::A2aResult<
                        Option<TaskPushNotificationConfig>,
                    >,
                > + Send
                + 'a,
        >,
    > {
        self.inner.get(task_id, id)
    }

    fn list<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = a2a_protocol_types::error::A2aResult<Vec<TaskPushNotificationConfig>>,
                > + Send
                + 'a,
        >,
    > {
        let fail = {
            let mut left = self.failures.lock().unwrap();
            let fail = *left > 0;
            *left = left.saturating_sub(1);
            fail
        };
        if fail {
            let error = (self.error)();
            return Box::pin(async move { Err(error) });
        }
        self.inner.list(task_id)
    }

    fn delete<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>> {
        self.inner.delete(task_id, id)
    }
}

#[derive(Default)]
struct Seen {
    outcomes: Mutex<Vec<String>>,
    retries: Mutex<Vec<(String, String)>>,
}

impl Metrics for Seen {
    fn on_push_delivery(&self, outcome: &str) {
        self.outcomes.lock().unwrap().push(outcome.to_owned());
    }

    fn on_store_retry(&self, operation: &str, cause: &str) {
        self.retries
            .lock()
            .unwrap()
            .push((operation.to_owned(), cause.to_owned()));
    }
}

#[derive(Default)]
struct Sends(AtomicUsize);

impl crate::push::PushSender for Sends {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

async fn store(failures: usize, error: fn() -> A2aError) -> FlakyList {
    let inner = InMemoryPushConfigStore::new();
    inner
        .set(
            TaskPushNotificationConfig::new(
                "t1".to_owned(),
                "https://hook.example.com/a".to_owned(),
            )
            .with_id("cfg".to_owned()),
        )
        .await
        .unwrap();
    FlakyList {
        inner,
        failures: Mutex::new(failures),
        error,
    }
}

fn unavailable() -> A2aError {
    A2aError::unavailable("database is restarting")
}

fn broken() -> A2aError {
    A2aError::internal("schema mismatch")
}

async fn push_once(store: &FlakyList, sends: &Sends, metrics: &Seen, retry: Duration) {
    let limits = HandlerLimits::default().with_store_write_retry(retry);
    deliver_push_bg(
        &TaskId::new("t1"),
        &make_status_event("t1", TaskState::Working),
        store,
        Some(sends),
        &limits,
        metrics,
    )
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_store_that_is_briefly_unavailable_is_waited_out() {
    let (sends, metrics) = (Sends::default(), Seen::default());
    let store = store(2, unavailable).await;
    push_once(&store, &sends, &metrics, Duration::from_secs(30)).await;

    assert_eq!(sends.0.load(Ordering::SeqCst), 1, "the webhook was tried");
    assert_eq!(
        *metrics.outcomes.lock().unwrap(),
        [push_outcome::DELIVERED],
        "and nothing was reported as failed"
    );
    assert_eq!(
        *metrics.retries.lock().unwrap(),
        [
            (
                persistence_operation::PUSH_CONFIG_LIST.to_owned(),
                "unavailable".to_owned()
            ),
            (
                persistence_operation::PUSH_CONFIG_LIST.to_owned(),
                "unavailable".to_owned()
            ),
        ],
        "two reads were retried, under the operation's own label"
    );
}

#[tokio::test(start_paused = true)]
async fn a_store_that_stays_unavailable_costs_the_event_its_webhooks_and_says_so() {
    let (sends, metrics) = (Sends::default(), Seen::default());
    let store = store(usize::MAX, unavailable).await;
    push_once(&store, &sends, &metrics, Duration::from_millis(400)).await;

    assert_eq!(sends.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        *metrics.outcomes.lock().unwrap(),
        [push_outcome::FAILED],
        "one failed push for the event, not one per config and not none"
    );
    assert!(
        !metrics.retries.lock().unwrap().is_empty(),
        "it waited before giving up"
    );
}

#[tokio::test(start_paused = true)]
async fn an_error_that_is_not_unavailability_is_reported_without_a_retry() {
    let (sends, metrics) = (Sends::default(), Seen::default());
    let store = store(usize::MAX, broken).await;
    push_once(&store, &sends, &metrics, Duration::from_secs(30)).await;

    assert_eq!(sends.0.load(Ordering::SeqCst), 0);
    assert_eq!(*metrics.outcomes.lock().unwrap(), [push_outcome::FAILED]);
    assert_eq!(
        metrics.retries.lock().unwrap().len(),
        0,
        "nothing to wait for"
    );
}

#[tokio::test(start_paused = true)]
async fn a_task_with_no_webhooks_reports_nothing() {
    let (sends, metrics) = (Sends::default(), Seen::default());
    let store = FlakyList {
        inner: InMemoryPushConfigStore::new(),
        failures: Mutex::new(0),
        error: broken,
    };
    push_once(&store, &sends, &metrics, Duration::from_secs(30)).await;
    assert_eq!(metrics.outcomes.lock().unwrap().len(), 0);
    assert_eq!(sends.0.load(Ordering::SeqCst), 0);
}
