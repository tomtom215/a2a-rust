// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests 61-90: E2E coverage gaps identified during dogfooding.
//!
//! Split into submodules by responsibility:
//! - [`batch_jsonrpc`]: Tests 61-66 — batch JSON-RPC (empty, single, mixed,
//!   streaming-in-batch, error-in-batch, large-batch)
//! - [`auth_and_cards`]: Tests 67-70 — auth rejection, extended card, dynamic
//!   cards, HTTP caching
//! - [`streaming_backpressure`]: Test 71 — backpressure / slow reader skips
//!   lagged events
//! - [`push_config`]: Tests 72-75 — push config limits, webhook validation,
//!   combined filters, metrics
//! - [`resilience`]: Tests 76-78 — timeout retryability, concurrent cancel,
//!   stale page token
//! - [`deep_dogfood`]: Tests 81-90 — deep probes: state transitions, executor
//!   errors, streaming completeness, metadata limits, artifact correctness,
//!   rapid throughput, terminal-state cancel, card validation, background sync
//! - [`feature_gated`]: Tests 91-92 — agent card signing (`signing` feature),
//!   `OtelMetrics` integration (`otel` feature)

pub mod auth_and_cards;
#[cfg(any(feature = "axum", feature = "sqlite"))]
pub mod axum_sqlite;
pub mod batch_jsonrpc;
pub mod deep_dogfood;
pub mod feature_gated;
pub mod push_config;
pub mod resilience;
pub mod streaming_backpressure;

// Re-export all test functions so callers can use `coverage_gaps::test_*`.
pub use auth_and_cards::*;
#[cfg(any(feature = "axum", feature = "sqlite"))]
pub use axum_sqlite::*;
pub use batch_jsonrpc::*;
pub use deep_dogfood::*;
#[cfg(any(feature = "signing", feature = "otel"))]
pub use feature_gated::*;
pub use push_config::*;
pub use resilience::*;
pub use streaming_backpressure::*;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::{AgentCardProducer, DynamicAgentCardHandler};
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::error::{A2aError, A2aResult, ErrorCode};

use super::{TestContext, TestResult};
use crate::helpers::make_send_params;
use crate::infrastructure::{
    bind_listener, serve_jsonrpc, AuditInterceptor, MetricsForward, TeamMetrics,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// POST raw JSON to a JSON-RPC endpoint and return `(status, body)`.
pub(crate) async fn post_raw(url: &str, body: &str) -> Result<(u16, String), String> {
    let uri: hyper::Uri = url.parse().map_err(|e| format!("bad URI: {e}"))?;

    let req = Request::builder()
        .method("POST")
        .uri(&uri)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body.to_owned())))
        .map_err(|e| format!("build request: {e}"))?;

    let host = uri.host().unwrap_or("127.0.0.1");
    let port = uri.port_u16().unwrap_or(80);
    let addr = format!("{host}:{port}");

    let tcp = tokio::net::TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let io = hyper_util::rt::TokioIo::new(tcp);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let resp = sender
        .send_request(req)
        .await
        .map_err(|e| format!("send: {e}"))?;
    let status = resp.status().as_u16();
    let body_bytes = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();
    Ok((status, String::from_utf8_lossy(&body_bytes).to_string()))
}

/// GET a URL and return `(status, headers_map, body)`.
pub(crate) async fn get_raw(
    url: &str,
    extra_headers: &[(&str, &str)],
) -> Result<(u16, Vec<(String, String)>, String), String> {
    let uri: hyper::Uri = url.parse().map_err(|e| format!("bad URI: {e}"))?;

    let mut builder = Request::builder().method("GET").uri(&uri);
    for (k, v) in extra_headers {
        builder = builder.header(*k, *v);
    }
    let req = builder
        .body(Full::new(Bytes::new()))
        .map_err(|e| format!("build request: {e}"))?;

    let host = uri.host().unwrap_or("127.0.0.1");
    let port = uri.port_u16().unwrap_or(80);
    let addr = format!("{host}:{port}");

    let tcp = tokio::net::TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let io = hyper_util::rt::TokioIo::new(tcp);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let resp = sender
        .send_request(req)
        .await
        .map_err(|e| format!("send: {e}"))?;
    let status = resp.status().as_u16();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_owned()))
        .collect();
    let body_bytes = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();
    Ok((
        status,
        headers,
        String::from_utf8_lossy(&body_bytes).to_string(),
    ))
}

pub(crate) fn jsonrpc_request(
    id: serde_json::Value,
    method: &str,
    params: serde_json::Value,
) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

pub(crate) fn send_message_params() -> serde_json::Value {
    let p = make_send_params("fn batch_test() {}");
    serde_json::to_value(&p).unwrap()
}
