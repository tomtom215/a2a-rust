// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Metrics hooks for observing handler activity.
//!
//! Implement [`Metrics`] to receive callbacks on requests, responses, errors,
//! latency, and queue depth changes. The default no-op implementation can be
//! overridden selectively.
//!
//! # Example
//!
//! ```rust,no_run
//! use a2a_protocol_server::metrics::Metrics;
//! use std::time::Duration;
//!
//! struct MyMetrics;
//!
//! impl Metrics for MyMetrics {
//!     fn on_request(&self, method: &str) {
//!         println!("request: {method}");
//!     }
//!     fn on_latency(&self, method: &str, duration: Duration) {
//!         println!("{method} took {duration:?}");
//!     }
//! }
//! ```

use std::sync::Arc;
use std::time::Duration;

/// Statistics about the HTTP connection pool.
///
/// Exposes hyper connection pool state for monitoring dashboards and alerts.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConnectionPoolStats {
    /// Number of active (in-use) connections.
    pub active_connections: u32,
    /// Number of idle connections waiting for reuse.
    pub idle_connections: u32,
    /// Total connections created since process start.
    pub total_connections_created: u64,
    /// Connections closed due to errors or timeouts.
    pub connections_closed: u64,
}

/// Trait for receiving metrics callbacks from the handler.
///
/// All methods have default no-op implementations so that consumers can
/// override only the callbacks they care about.
pub trait Metrics: Send + Sync + 'static {
    /// Called when a request is received, before processing.
    fn on_request(&self, _method: &str) {}

    /// Called when a response is successfully sent.
    fn on_response(&self, _method: &str) {}

    /// Called when a request results in an error.
    fn on_error(&self, _method: &str, _error: &str) {}

    /// Called when a request completes (successfully or not) with the wall-clock
    /// duration from receipt to response.
    ///
    /// This is the #1 production observability metric — use it to feed
    /// histograms, percentile trackers, or SLO dashboards.
    fn on_latency(&self, _method: &str, _duration: Duration) {}

    /// Called when the number of active event queues changes.
    fn on_queue_depth_change(&self, _active_queues: usize) {}

    /// Called with connection pool statistics when available.
    ///
    /// Useful for monitoring connection pool health and detecting exhaustion.
    fn on_connection_pool_stats(&self, _stats: &ConnectionPoolStats) {}
}

/// A no-op [`Metrics`] implementation that discards all events.
#[derive(Debug, Default)]
pub struct NoopMetrics;

impl Metrics for NoopMetrics {}

/// Blanket implementation: `Arc<T>` implements [`Metrics`] if `T` does.
///
/// This eliminates the need for wrapper types like `MetricsForward` when
/// sharing a metrics instance across multiple handlers or tasks.
impl<T: Metrics + ?Sized> Metrics for Arc<T> {
    fn on_request(&self, method: &str) {
        (**self).on_request(method);
    }

    fn on_response(&self, method: &str) {
        (**self).on_response(method);
    }

    fn on_error(&self, method: &str, error: &str) {
        (**self).on_error(method, error);
    }

    fn on_latency(&self, method: &str, duration: Duration) {
        (**self).on_latency(method, duration);
    }

    fn on_queue_depth_change(&self, active_queues: usize) {
        (**self).on_queue_depth_change(active_queues);
    }

    fn on_connection_pool_stats(&self, stats: &ConnectionPoolStats) {
        (**self).on_connection_pool_stats(stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A test metrics implementation that records which methods were called.
    struct RecordingMetrics {
        request_count: AtomicU64,
        response_count: AtomicU64,
        error_count: AtomicU64,
        latency_count: AtomicU64,
        queue_depth_count: AtomicU64,
        pool_stats_count: AtomicU64,
    }

    impl RecordingMetrics {
        fn new() -> Self {
            Self {
                request_count: AtomicU64::new(0),
                response_count: AtomicU64::new(0),
                error_count: AtomicU64::new(0),
                latency_count: AtomicU64::new(0),
                queue_depth_count: AtomicU64::new(0),
                pool_stats_count: AtomicU64::new(0),
            }
        }
    }

    impl Metrics for RecordingMetrics {
        fn on_request(&self, _method: &str) {
            self.request_count.fetch_add(1, Ordering::Relaxed);
        }
        fn on_response(&self, _method: &str) {
            self.response_count.fetch_add(1, Ordering::Relaxed);
        }
        fn on_error(&self, _method: &str, _error: &str) {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }
        fn on_latency(&self, _method: &str, _duration: Duration) {
            self.latency_count.fetch_add(1, Ordering::Relaxed);
        }
        fn on_queue_depth_change(&self, _active_queues: usize) {
            self.queue_depth_count.fetch_add(1, Ordering::Relaxed);
        }
        fn on_connection_pool_stats(&self, _stats: &ConnectionPoolStats) {
            self.pool_stats_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn arc_delegates_on_request() {
        let inner = Arc::new(RecordingMetrics::new());
        let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
        arc_metrics.on_request("test");
        assert_eq!(inner.request_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn arc_delegates_on_response() {
        let inner = Arc::new(RecordingMetrics::new());
        let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
        arc_metrics.on_response("test");
        assert_eq!(inner.response_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn arc_delegates_on_error() {
        let inner = Arc::new(RecordingMetrics::new());
        let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
        arc_metrics.on_error("test", "err");
        assert_eq!(inner.error_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn arc_delegates_on_latency() {
        let inner = Arc::new(RecordingMetrics::new());
        let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
        arc_metrics.on_latency("test", Duration::from_millis(10));
        assert_eq!(inner.latency_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn arc_delegates_on_queue_depth_change() {
        let inner = Arc::new(RecordingMetrics::new());
        let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
        arc_metrics.on_queue_depth_change(5);
        assert_eq!(inner.queue_depth_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn arc_delegates_on_connection_pool_stats() {
        let inner = Arc::new(RecordingMetrics::new());
        let arc_metrics: Arc<RecordingMetrics> = Arc::clone(&inner);
        arc_metrics.on_connection_pool_stats(&ConnectionPoolStats::default());
        assert_eq!(inner.pool_stats_count.load(Ordering::Relaxed), 1);
    }
}
