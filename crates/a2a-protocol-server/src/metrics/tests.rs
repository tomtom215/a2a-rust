// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for [`Metrics`](super::Metrics), split out of `mod.rs` for the
//! 500-line limit `CONTRIBUTING.md` sets.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// A test metrics implementation that records which methods were called.
struct RecordingMetrics {
    requests: AtomicU64,
    responses: AtomicU64,
    errors: AtomicU64,
    latencies: AtomicU64,
    queue_depths: AtomicU64,
    pool_stats: AtomicU64,
}

impl RecordingMetrics {
    fn new() -> Self {
        Self {
            requests: AtomicU64::new(0),
            responses: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            latencies: AtomicU64::new(0),
            queue_depths: AtomicU64::new(0),
            pool_stats: AtomicU64::new(0),
        }
    }
}

impl Metrics for RecordingMetrics {
    fn on_request(&self, _method: &str) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }
    fn on_response(&self, _method: &str) {
        self.responses.fetch_add(1, Ordering::Relaxed);
    }
    fn on_error(&self, _method: &str, _error: &str) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
    fn on_latency(&self, _method: &str, _duration: Duration) {
        self.latencies.fetch_add(1, Ordering::Relaxed);
    }
    fn on_queue_depth_change(&self, _active_queues: usize) {
        self.queue_depths.fetch_add(1, Ordering::Relaxed);
    }
    fn on_connection_pool_stats(&self, _stats: &ConnectionPoolStats) {
        self.pool_stats.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn arc_delegates_on_request() {
    let inner = Arc::new(RecordingMetrics::new());
    let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
    arc_metrics.on_request("test");
    assert_eq!(inner.requests.load(Ordering::Relaxed), 1);
}

#[test]
fn arc_delegates_on_response() {
    let inner = Arc::new(RecordingMetrics::new());
    let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
    arc_metrics.on_response("test");
    assert_eq!(inner.responses.load(Ordering::Relaxed), 1);
}

#[test]
fn arc_delegates_on_error() {
    let inner = Arc::new(RecordingMetrics::new());
    let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
    arc_metrics.on_error("test", "err");
    assert_eq!(inner.errors.load(Ordering::Relaxed), 1);
}

#[test]
fn arc_delegates_on_latency() {
    let inner = Arc::new(RecordingMetrics::new());
    let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
    arc_metrics.on_latency("test", Duration::from_millis(10));
    assert_eq!(inner.latencies.load(Ordering::Relaxed), 1);
}

#[test]
fn arc_delegates_on_queue_depth_change() {
    let inner = Arc::new(RecordingMetrics::new());
    let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
    arc_metrics.on_queue_depth_change(5);
    assert_eq!(inner.queue_depths.load(Ordering::Relaxed), 1);
}

#[test]
fn arc_delegates_on_connection_pool_stats() {
    let inner = Arc::new(RecordingMetrics::new());
    let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
    arc_metrics.on_connection_pool_stats(&ConnectionPoolStats::default());
    assert_eq!(inner.pool_stats.load(Ordering::Relaxed), 1);
}

#[test]
fn arc_delegates_on_rpc_call() {
    #[derive(Default)]
    struct Seen(std::sync::Mutex<Vec<String>>);
    impl Metrics for Seen {
        fn on_rpc_call(&self, call: &RpcCall<'_>) {
            self.0.lock().unwrap().push(format!(
                "{} {:?} {:?}",
                call.system, call.method, call.error_type
            ));
        }
    }
    let seen = Arc::new(Seen::default());
    let via_arc: Arc<dyn Metrics> = Arc::clone(&seen) as Arc<dyn Metrics>;
    via_arc.on_rpc_call(&RpcCall {
        system: "grpc",
        method: Some("lf.a2a.v1.A2AService/GetTask"),
        duration: Duration::from_millis(3),
        status_code: Some("NOT_FOUND"),
        error_type: Some("NOT_FOUND"),
    });
    assert_eq!(
        *seen.0.lock().unwrap(),
        ["grpc Some(\"lf.a2a.v1.A2AService/GetTask\") Some(\"NOT_FOUND\")"]
    );
}

#[test]
fn arc_delegates_on_persistence_error_and_on_push_delivery() {
    #[derive(Default)]
    struct Seen {
        persistence: std::sync::Mutex<Vec<(String, String)>>,
        push: std::sync::Mutex<Vec<String>>,
        retries: std::sync::Mutex<Vec<(String, String)>>,
    }
    impl Metrics for Seen {
        fn on_store_retry(&self, operation: &str, cause: &str) {
            self.retries
                .lock()
                .unwrap()
                .push((operation.to_owned(), cause.to_owned()));
        }
        fn on_persistence_error(&self, operation: &str, error_kind: &str) {
            self.persistence
                .lock()
                .unwrap()
                .push((operation.to_owned(), error_kind.to_owned()));
        }
        fn on_push_delivery(&self, outcome: &str) {
            self.push.lock().unwrap().push(outcome.to_owned());
        }
    }
    let seen = Arc::new(Seen::default());
    let via_arc: Arc<dyn Metrics> = Arc::clone(&seen) as Arc<dyn Metrics>;
    // Called on the `Arc<dyn Metrics>` itself, which is what a handler holds.
    Metrics::on_persistence_error(&via_arc, persistence_operation::QUEUE_HANDOFF, "closed");
    Metrics::on_push_delivery(&via_arc, push_outcome::SKIPPED);
    assert_eq!(
        *seen.persistence.lock().unwrap(),
        vec![(
            persistence_operation::QUEUE_HANDOFF.to_owned(),
            "closed".to_owned()
        )],
        "an Arc must forward persistence errors, not default them to nothing"
    );
    assert_eq!(
        *seen.push.lock().unwrap(),
        vec![push_outcome::SKIPPED.to_owned()]
    );
    Metrics::on_store_retry(&via_arc, "statement", "connection_lost");
    assert_eq!(
        *seen.retries.lock().unwrap(),
        vec![("statement".to_owned(), "connection_lost".to_owned())],
        "an Arc must forward store retries"
    );
}

/// Logs `"{name} {callback}"` for every callback into a log shared with the
/// other recorder, so the order across the two is visible.
struct Logged {
    name: &'static str,
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Logged {
    fn note(&self, callback: &str) {
        self.log
            .lock()
            .unwrap()
            .push(format!("{} {callback}", self.name));
    }
}

impl Metrics for Logged {
    fn on_request(&self, _: &str) {
        self.note("request");
    }
    fn on_response(&self, _: &str) {
        self.note("response");
    }
    fn on_error(&self, _: &str, _: &str) {
        self.note("error");
    }
    fn on_latency(&self, _: &str, _: Duration) {
        self.note("latency");
    }
    fn on_queue_depth_change(&self, _: usize) {
        self.note("queue_depth");
    }
    fn on_connection_pool_stats(&self, _: &ConnectionPoolStats) {
        self.note("pool_stats");
    }
    fn on_persistence_error(&self, _: &str, _: &str) {
        self.note("persistence_error");
    }
    fn on_store_retry(&self, _: &str, _: &str) {
        self.note("store_retry");
    }
    fn on_push_delivery(&self, _: &str) {
        self.note("push_delivery");
    }
    fn on_rpc_call(&self, _: &RpcCall<'_>) {
        self.note("rpc_call");
    }
    fn on_executor_run(&self, _: &str, _: Duration) {
        self.note("executor_run");
    }
}

/// Every one of the trait's eleven callbacks reaches both sides, the first
/// side first. A callback `Tee` forgot would default to nothing on both.
#[test]
fn a_tee_forwards_every_callback_to_both_in_order() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let tee = Tee {
        first: Logged {
            name: "first",
            log: Arc::clone(&log),
        },
        second: Logged {
            name: "second",
            log: Arc::clone(&log),
        },
    };
    tee.on_request("m");
    tee.on_response("m");
    tee.on_error("m", "e");
    tee.on_latency("m", Duration::ZERO);
    tee.on_queue_depth_change(1);
    tee.on_connection_pool_stats(&ConnectionPoolStats::default());
    tee.on_persistence_error("op", "kind");
    tee.on_store_retry("statement", "connection_lost");
    tee.on_push_delivery("delivered");
    tee.on_rpc_call(&RpcCall {
        system: "jsonrpc",
        method: None,
        duration: Duration::ZERO,
        status_code: None,
        error_type: None,
    });
    tee.on_executor_run("completed", Duration::ZERO);
    let expected: Vec<String> = [
        "request",
        "response",
        "error",
        "latency",
        "queue_depth",
        "pool_stats",
        "persistence_error",
        "store_retry",
        "push_delivery",
        "rpc_call",
        "executor_run",
    ]
    .iter()
    .flat_map(|c| [format!("first {c}"), format!("second {c}")])
    .collect();
    assert_eq!(*log.lock().unwrap(), expected);
}
