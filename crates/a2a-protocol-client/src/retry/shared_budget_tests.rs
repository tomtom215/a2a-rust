// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A retry paid from a pool's per-origin budget as well as the client's own
//! (ADR 0050 decision 6), and an open circuit as a safe retry.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::budget::Ledger;
use super::{RetryBudget, RetryPolicy, RetryTransport};
use crate::error::{ClientError, ClientResult};
use crate::origin::Origins;
use crate::streaming::EventStream;
use crate::transport::Transport;

/// Answers each call with the next of `script`, then with a timeout.
struct Scripted {
    script: Mutex<Vec<ClientResult<serde_json::Value>>>,
    calls: Arc<AtomicUsize>,
}

impl Transport for Scripted {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut script = self.script.lock().unwrap();
        let next = if script.is_empty() {
            Err(ClientError::Timeout("t".into()))
        } else {
            script.remove(0)
        };
        drop(script);
        Box::pin(async move { next })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async { Err(ClientError::Transport("not used".into())) })
    }
}

fn policy() -> RetryPolicy {
    RetryPolicy::default()
        .with_max_retries(3)
        .with_initial_backoff(Duration::from_millis(1))
        .with_max_backoff(Duration::from_millis(1))
}

/// A transport that always times out, under `policy`, paying from a pool
/// budget of `shared`; and its call count.
fn failing(policy: RetryPolicy, shared: RetryBudget) -> (RetryTransport, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let inner = Scripted {
        script: Mutex::new(Vec::new()),
        calls: Arc::clone(&calls),
    };
    let origin = Origins::new(None, None, Some(shared)).get("http://a.test:80");
    (
        RetryTransport::new(Box::new(inner), policy).with_shared_budget(origin),
        calls,
    )
}

async fn get_task(t: &RetryTransport) -> ClientResult<serde_json::Value> {
    t.send_request("GetTask", serde_json::json!({"id": "t"}), &HashMap::new())
        .await
}

/// The pool's budget alone stops retries: one in reserve, nothing earned.
#[tokio::test(start_paused = true)]
async fn the_pools_budget_pays_for_retries() {
    let (t, calls) = failing(policy(), RetryBudget::new(0.0, 0.0).with_reserve(1.0));
    get_task(&t).await.expect_err("times out");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "one attempt, one paid retry"
    );
}

/// Each call earns the pool's budget its share: at half a retry per call,
/// the calls make 2, 1 and 2 attempts.
#[tokio::test(start_paused = true)]
async fn calls_earn_the_pools_budget() {
    let (t, calls) = failing(policy(), RetryBudget::new(0.5, 0.0).with_reserve(1.0));
    let mut made = Vec::new();
    for _ in 0..3 {
        get_task(&t).await.expect_err("times out");
        made.push(calls.swap(0, Ordering::SeqCst));
    }
    assert_eq!(made, [2, 1, 2]);
}

/// A retry the pool will not pay for is not made, and the client's own
/// budget gets back what it paid: own reserve 3, the pool's 1. The first
/// retry costs one of each; the second is refused by the pool, so the client
/// keeps 2.
#[tokio::test(start_paused = true)]
async fn a_refused_retry_is_refunded() {
    let own = RetryBudget::new(0.0, 0.0).with_reserve(3.0);
    let (t, calls) = failing(
        policy().with_budget(own),
        RetryBudget::new(0.0, 0.0).with_reserve(1.0),
    );
    get_task(&t).await.expect_err("times out");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let ledger = t.ledger.as_ref().expect("its own budget");
    assert!(ledger.withdraw() && ledger.withdraw(), "two retries left");
    assert!(!ledger.withdraw());
}

/// A refund restores a retry, and never past the reserve.
#[test]
fn a_refund_is_capped_at_the_reserve() {
    let ledger = Ledger::new(RetryBudget::new(0.0, 0.0).with_reserve(1.0));
    ledger.refund();
    assert!(ledger.withdraw());
    assert!(!ledger.withdraw());
    ledger.refund();
    assert!(ledger.withdraw());
}

/// An open circuit refused the call before it was sent, so even a send
/// without an idempotency key is retried.
#[tokio::test(start_paused = true)]
async fn a_send_refused_by_an_open_circuit_is_retried() {
    let calls = Arc::new(AtomicUsize::new(0));
    let inner = Scripted {
        script: Mutex::new(vec![
            Err(ClientError::CircuitOpen {
                origin: "http://a.test:80".into(),
                retry_after: None,
            }),
            Ok(serde_json::json!({"ok": true})),
        ]),
        calls: Arc::clone(&calls),
    };
    let t = RetryTransport::new(Box::new(inner), policy());
    let sent = t
        .send_request(
            "SendMessage",
            serde_json::json!({"message": {}}),
            &HashMap::new(),
        )
        .await
        .expect("retried");
    assert_eq!(sent, serde_json::json!({"ok": true}));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
