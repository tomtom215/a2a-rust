// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! E2E test suite for the agent team demo.
//!
//! Tests are split across submodules by category:
//! - [`basic`]: Tests 1-10 — core send/stream/REST/JSON-RPC paths
//! - [`lifecycle`]: Tests 11-20 — orchestration, metadata, cancel, agent cards
//! - [`edge_cases`]: Tests 21-30 — error paths, concurrency, metrics, CRUD
//! - [`stress`]: Tests 31-40 — stress, durability, observability, event ordering
//! - [`dogfood`]: Tests 41-50 — SDK gaps and regressions found during review
//! - [`coverage_gaps`]: Tests 61-90 — E2E coverage gaps (batch JSON-RPC, auth,
//!   dynamic/extended cards, caching, backpressure, timeout retryability,
//!   concurrent cancels, stale pagination, deep dogfood probes)

pub mod basic;
pub mod coverage_gaps;
pub mod dogfood;
pub mod edge_cases;
pub mod lifecycle;
pub mod stress;
pub mod transport;

use std::net::SocketAddr;
use std::sync::Arc;

use crate::infrastructure::{TeamMetrics, WebhookReceiver};

// ─── Test result ─────────────────────────────────────────────────────────────

/// Outcome of a single E2E test.
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u128,
    pub details: String,
}

impl TestResult {
    /// Create a passing result.
    pub fn pass(name: &str, duration_ms: u128, details: &str) -> Self {
        Self {
            name: name.to_owned(),
            passed: true,
            duration_ms,
            details: details.to_owned(),
        }
    }

    /// Create a failing result.
    pub fn fail(name: &str, duration_ms: u128, details: &str) -> Self {
        Self {
            name: name.to_owned(),
            passed: false,
            duration_ms,
            details: details.to_owned(),
        }
    }
}

// ─── Test context ────────────────────────────────────────────────────────────

/// Everything an individual test function needs to run.
///
/// Built once in `main()` after all servers are started, then passed by
/// reference to every `pub async fn test_*(ctx: &TestContext) -> TestResult`.
pub struct TestContext {
    /// Base URL for the CodeAnalyzer agent (JSON-RPC).
    pub analyzer_url: String,
    /// Base URL for the BuildMonitor agent (REST).
    pub build_url: String,
    /// Base URL for the HealthMonitor agent (JSON-RPC).
    pub health_url: String,
    /// Base URL for the Coordinator agent (REST).
    pub coordinator_url: String,

    /// Socket address of the webhook receiver (for push notification tests).
    pub webhook_addr: SocketAddr,
    /// Webhook receiver instance (for draining received payloads).
    #[allow(dead_code)]
    pub webhook_receiver: WebhookReceiver,

    /// Metrics for CodeAnalyzer.
    pub analyzer_metrics: Arc<TeamMetrics>,
    /// Metrics for BuildMonitor.
    pub build_metrics: Arc<TeamMetrics>,
    /// Metrics for HealthMonitor.
    pub health_metrics: Arc<TeamMetrics>,
    /// Metrics for Coordinator.
    pub coordinator_metrics: Arc<TeamMetrics>,

    /// Base URL for the gRPC CodeAnalyzer agent (gRPC transport).
    #[cfg(feature = "grpc")]
    pub grpc_analyzer_url: String,
    /// Metrics for the gRPC CodeAnalyzer agent.
    #[cfg(feature = "grpc")]
    #[allow(dead_code)]
    pub grpc_analyzer_metrics: Arc<TeamMetrics>,
}
