// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Metrics hooks for observing handler activity.
//!
//! Implement [`Metrics`] to receive callbacks on requests, responses, errors,
//! and queue depth changes. The default no-op implementation can be overridden
//! selectively.
//!
//! # Example
//!
//! ```rust,no_run
//! use a2a_server::metrics::Metrics;
//!
//! struct MyMetrics;
//!
//! impl Metrics for MyMetrics {
//!     fn on_request(&self, method: &str) {
//!         println!("request: {method}");
//!     }
//! }
//! ```

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

    /// Called when the number of active event queues changes.
    fn on_queue_depth_change(&self, _active_queues: usize) {}
}

/// A no-op [`Metrics`] implementation that discards all events.
#[derive(Debug, Default)]
pub struct NoopMetrics;

impl Metrics for NoopMetrics {}
