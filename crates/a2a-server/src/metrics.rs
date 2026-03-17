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
