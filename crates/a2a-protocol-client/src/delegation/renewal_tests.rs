// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! When a lease renewal is due: at once for a child already named, a third
//! of the lease after the start for one not yet named, and every third of it
//! after that.
//!
//! The stub counts the `GetTask` calls a renewal makes; the clock is paused
//! and moved by exact amounts.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_types::{Task, TaskState, TaskStatus};

use super::*;
use crate::ClientBuilder;
use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

/// A lease of thirty seconds, renewed every ten.
const TTL: Duration = Duration::from_secs(30);
const THIRD: Duration = Duration::from_secs(10);
const NANOSECOND: Duration = Duration::from_nanos(1);

/// Answers every `GetTask` with a task in `state`, counting them.
struct Counting {
    calls: Arc<AtomicUsize>,
    state: TaskState,
}

impl Transport for Counting {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let task = Task::new("t", "c", TaskStatus::new(self.state));
        Box::pin(async move { serde_json::to_value(task).map_err(ClientError::Serialization) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async { Err(ClientError::Transport("no streams here".into())) })
    }
}

fn client(calls: &Arc<AtomicUsize>, state: TaskState) -> Arc<A2aClient> {
    Arc::new(
        ClientBuilder::new("http://stub.invalid")
            .with_custom_transport(Counting {
                calls: Arc::clone(calls),
                state,
            })
            .build()
            .expect("client"),
    )
}

fn calls_made(calls: &Arc<AtomicUsize>) -> usize {
    calls.load(Ordering::SeqCst)
}

/// Lets the renewal task run whatever is due now.
async fn settle() {
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}

/// Moves the clock on by `by`, and lets the task run.
async fn elapse(by: Duration) {
    tokio::time::advance(by).await;
    settle().await;
}

#[tokio::test(start_paused = true)]
async fn a_child_already_named_is_renewed_at_once_and_then_every_third_of_the_lease() {
    let calls = Arc::new(AtomicUsize::new(0));
    let _renewal = Renewal::spawn(
        client(&calls, TaskState::Working),
        None,
        TTL,
        &Origin::current(),
        Some("t".to_owned()),
    );

    settle().await;
    assert_eq!(calls_made(&calls), 1, "at once");

    elapse(THIRD.saturating_sub(NANOSECOND)).await;
    assert_eq!(calls_made(&calls), 1, "a nanosecond short of a third");
    elapse(NANOSECOND).await;
    assert_eq!(calls_made(&calls), 2, "a third after the first");
    elapse(THIRD).await;
    assert_eq!(calls_made(&calls), 3, "and a third after that");
}

#[tokio::test(start_paused = true)]
async fn a_child_not_yet_named_is_first_renewed_a_third_of_the_lease_after_the_start() {
    let calls = Arc::new(AtomicUsize::new(0));
    let renewal = Renewal::spawn(
        client(&calls, TaskState::Working),
        None,
        TTL,
        &Origin::current(),
        None,
    );
    // The first event names the child at the start, as `Delegation` does.
    *renewal
        .id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some("t".to_owned());

    settle().await;
    assert_eq!(calls_made(&calls), 0, "its lease was granted a moment ago");
    elapse(THIRD.saturating_sub(NANOSECOND)).await;
    assert_eq!(calls_made(&calls), 0, "a nanosecond short of a third");
    elapse(NANOSECOND).await;
    assert_eq!(calls_made(&calls), 1, "a third after the start");
    elapse(THIRD).await;
    assert_eq!(calls_made(&calls), 2);
}

#[tokio::test(start_paused = true)]
async fn a_child_named_after_the_first_tick_is_renewed_at_the_next_one() {
    let calls = Arc::new(AtomicUsize::new(0));
    let renewal = Renewal::spawn(
        client(&calls, TaskState::Working),
        None,
        TTL,
        &Origin::current(),
        None,
    );
    // The task registers its first wait when it first runs, so let it run
    // before the clock moves: its first tick is at 10 s.
    settle().await;

    elapse(THIRD).await;
    assert_eq!(calls_made(&calls), 0, "nothing to renew yet");

    elapse(Duration::from_secs(5)).await;
    *renewal
        .id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some("t".to_owned());
    elapse(Duration::from_secs(5).saturating_sub(NANOSECOND)).await;
    assert_eq!(calls_made(&calls), 0, "the second tick is at 20 s");
    elapse(NANOSECOND).await;
    assert_eq!(calls_made(&calls), 1, "renewed at the second tick");
}

#[tokio::test(start_paused = true)]
async fn renewals_end_once_a_renewal_finds_the_child_finished() {
    let calls = Arc::new(AtomicUsize::new(0));
    let _renewal = Renewal::spawn(
        client(&calls, TaskState::Completed),
        None,
        TTL,
        &Origin::current(),
        Some("t".to_owned()),
    );

    settle().await;
    assert_eq!(calls_made(&calls), 1);
    elapse(THIRD * 10).await;
    assert_eq!(calls_made(&calls), 1, "none after the child finished");
}

#[tokio::test(start_paused = true)]
async fn dropping_the_renewal_stops_it() {
    let calls = Arc::new(AtomicUsize::new(0));
    let renewal = Renewal::spawn(
        client(&calls, TaskState::Working),
        None,
        TTL,
        &Origin::current(),
        Some("t".to_owned()),
    );
    settle().await;
    assert_eq!(calls_made(&calls), 1);

    drop(renewal);
    elapse(THIRD * 10).await;
    assert_eq!(calls_made(&calls), 1, "none after the handle was dropped");
}

#[tokio::test(start_paused = true)]
async fn the_tenant_is_sent_with_each_renewal() {
    // `Renewal` does not expose the query it sends; this is the one place
    // the tenant is checked, through the stub seeing the request's params.
    struct Tenants(Arc<std::sync::Mutex<Vec<Option<String>>>>);
    impl Transport for Tenants {
        fn send_request<'a>(
            &'a self,
            _method: &'a str,
            params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
            self.0.lock().unwrap().push(
                params
                    .get("tenant")
                    .and_then(|t| t.as_str())
                    .map(str::to_owned),
            );
            let task = Task::new("t", "c", TaskStatus::new(TaskState::Working));
            Box::pin(async move { serde_json::to_value(task).map_err(ClientError::Serialization) })
        }

        fn send_streaming_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
            Box::pin(async { Err(ClientError::Transport("no streams here".into())) })
        }
    }

    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let client = Arc::new(
        ClientBuilder::new("http://stub.invalid")
            .with_custom_transport(Tenants(Arc::clone(&seen)))
            .build()
            .expect("client"),
    );
    let _renewal = Renewal::spawn(
        client,
        Some("acme".to_owned()),
        TTL,
        &Origin::current(),
        Some("t".to_owned()),
    );

    settle().await;
    assert_eq!(*seen.lock().unwrap(), vec![Some("acme".to_owned())]);
}
