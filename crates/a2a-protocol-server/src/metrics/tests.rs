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
    }
    impl Metrics for Seen {
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
}
