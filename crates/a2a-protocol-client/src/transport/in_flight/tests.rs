// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The cap counts the calls that wait for a response, refuses the next one
//! up front, and gives a permit back however a call ends.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;
use crate::retry::{RetryPolicy, RetryTransport};

/// Answers when the test releases it, counting the calls that reached it.
struct Held {
    entered: Arc<AtomicUsize>,
    gate: Arc<Semaphore>,
    /// What a released call answers.
    answer: fn() -> ClientResult<serde_json::Value>,
}

impl Held {
    fn new(
        answer: fn() -> ClientResult<serde_json::Value>,
    ) -> (Self, Arc<AtomicUsize>, Arc<Semaphore>) {
        let entered = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Semaphore::new(0));
        (
            Self {
                entered: Arc::clone(&entered),
                gate: Arc::clone(&gate),
                answer,
            },
            entered,
            gate,
        )
    }
}

// The signature is the fn pointer `Held` is given: it must return a `Result`.
#[allow(clippy::unnecessary_wraps)]
fn ok() -> ClientResult<serde_json::Value> {
    Ok(serde_json::json!({"ok": true}))
}

fn broken() -> ClientResult<serde_json::Value> {
    Err(ClientError::Transport("the peer is gone".into()))
}

impl Transport for Held {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            self.gate.acquire().await.expect("open").forget();
            (self.answer)()
        })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            self.gate.acquire().await.expect("open").forget();
            Err(ClientError::Transport("no stream in this test".into()))
        })
    }
}

fn call(
    transport: &Arc<InFlightTransport>,
) -> tokio::task::JoinHandle<ClientResult<serde_json::Value>> {
    let transport = Arc::clone(transport);
    tokio::spawn(async move {
        transport
            .send_request("GetTask", serde_json::json!({}), &HashMap::new())
            .await
    })
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..1_000 {
        if done() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("never happened: {what}");
}

fn refused(limit: usize) -> impl Fn(&ClientError) -> bool {
    move |e| matches!(e, ClientError::TooManyPendingRequests { limit: l } if *l == limit)
}

#[tokio::test]
async fn the_call_after_the_cap_is_refused_before_it_is_sent_and_the_cap_calls_are_not() {
    for limit in [1, 3] {
        let (inner, entered, gate) = Held::new(ok);
        let transport = Arc::new(InFlightTransport::new(Box::new(inner), limit));
        let held: Vec<_> = (0..limit).map(|_| call(&transport)).collect();
        until("the cap's calls have reached the peer", || {
            entered.load(Ordering::SeqCst) == limit
        })
        .await;

        let over = call(&transport).await.expect("joined");
        assert!(refused(limit)(&over.expect_err("refused")), "limit {limit}");
        assert_eq!(entered.load(Ordering::SeqCst), limit, "it was not sent");

        gate.add_permits(limit);
        for h in held {
            h.await.expect("joined").expect("answered");
        }
    }
}

#[tokio::test]
async fn a_finished_call_frees_its_place() {
    let (inner, entered, gate) = Held::new(ok);
    let transport = Arc::new(InFlightTransport::new(Box::new(inner), 1));
    for round in 1..=3 {
        let held = call(&transport);
        until("the call reached the peer", || {
            entered.load(Ordering::SeqCst) == round
        })
        .await;
        gate.add_permits(1);
        held.await.expect("joined").expect("answered");
    }
}

#[tokio::test]
async fn a_call_that_failed_frees_its_place_too() {
    let (inner, entered, gate) = Held::new(broken);
    gate.add_permits(10);
    let transport = Arc::new(InFlightTransport::new(Box::new(inner), 1));
    for _ in 0..5 {
        let error = call(&transport).await.expect("joined").expect_err("failed");
        assert!(matches!(error, ClientError::Transport(_)), "{error:?}");
    }
    assert_eq!(entered.load(Ordering::SeqCst), 5, "every call was sent");
}

#[tokio::test]
async fn a_call_that_is_dropped_frees_its_place() {
    let (inner, entered, gate) = Held::new(ok);
    let transport = Arc::new(InFlightTransport::new(Box::new(inner), 1));
    let abandoned = call(&transport);
    until("it reached the peer", || {
        entered.load(Ordering::SeqCst) == 1
    })
    .await;
    abandoned.abort();
    let _ = abandoned.await;

    gate.add_permits(1);
    let next = call(&transport).await.expect("joined");
    next.expect("accepted: the abandoned call let go");
}

#[tokio::test]
async fn a_streaming_start_takes_a_place_while_it_opens_and_gives_it_back() {
    let (inner, entered, gate) = Held::new(ok);
    let transport = Arc::new(InFlightTransport::new(Box::new(inner), 1));
    let opening = {
        let transport = Arc::clone(&transport);
        tokio::spawn(async move {
            transport
                .send_streaming_request("SubscribeToTask", serde_json::json!({}), &HashMap::new())
                .await
                .err()
        })
    };
    until("the start reached the peer", || {
        entered.load(Ordering::SeqCst) == 1
    })
    .await;

    let over = call(&transport)
        .await
        .expect("joined")
        .expect_err("refused");
    assert!(refused(1)(&over));

    let over_stream = transport
        .send_streaming_request("SubscribeToTask", serde_json::json!({}), &HashMap::new())
        .await
        .expect_err("refused");
    assert!(
        refused(1)(&over_stream),
        "a second start is refused as well"
    );
    assert_eq!(entered.load(Ordering::SeqCst), 1);

    gate.add_permits(1);
    opening.await.expect("joined");
    gate.add_permits(1);
    call(&transport).await.expect("joined").expect("room again");
}

#[tokio::test(start_paused = true)]
async fn with_a_retry_policy_a_refused_call_waits_for_room_and_goes_through() {
    let (inner, entered, gate) = Held::new(ok);
    let capped = InFlightTransport::new(Box::new(inner), 1);
    let policy = RetryPolicy::default()
        .with_initial_backoff(Duration::from_millis(100))
        .with_backoff_multiplier(1.0);
    let transport = Arc::new(RetryTransport::new(Box::new(capped), policy));

    let first = {
        let transport = Arc::clone(&transport);
        tokio::spawn(async move {
            transport
                .send_request("GetTask", serde_json::json!({}), &HashMap::new())
                .await
        })
    };
    until("the first call reached the peer", || {
        entered.load(Ordering::SeqCst) == 1
    })
    .await;

    // The second call is refused, backs off, and is refused again until the
    // first answers; it never reaches the peer in the meantime.
    let second = {
        let transport = Arc::clone(&transport);
        tokio::spawn(async move {
            transport
                .send_request("GetTask", serde_json::json!({}), &HashMap::new())
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(entered.load(Ordering::SeqCst), 1, "still waiting for room");
    gate.add_permits(1);
    first.await.expect("joined").expect("answered");
    gate.add_permits(1);
    second
        .await
        .expect("joined")
        .expect("answered after backing off");
    assert_eq!(entered.load(Ordering::SeqCst), 2);
}

#[test]
fn a_cap_of_zero_is_refused_when_the_client_is_built() {
    let error = crate::ClientBuilder::new("http://127.0.0.1:9")
        .with_max_in_flight(0)
        .build()
        .expect_err("refused");
    match error {
        ClientError::Config(message) => assert!(message.contains("max_in_flight"), "{message}"),
        other => panic!("expected a configuration error, got {other:?}"),
    }
    crate::ClientBuilder::new("http://127.0.0.1:9")
        .with_max_in_flight(1)
        .build()
        .expect("a cap of one is allowed");
}
