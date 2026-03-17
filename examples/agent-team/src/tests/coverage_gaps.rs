// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests 61-71: E2E coverage gaps identified during dogfooding.
//!
//! Each test in this module closes one of the gaps listed in
//! `book/src/deployment/dogfooding-open-issues.md`:
//!
//! - **61-66**: Batch JSON-RPC (empty, single, mixed, streaming-in-batch)
//! - **67**: Real auth rejection via interceptor
//! - **68**: Extended agent card via JSON-RPC
//! - **69**: Dynamic agent cards with `DynamicAgentCardHandler`
//! - **70**: Agent card HTTP caching (ETag, 304 Not Modified)
//! - **71**: Backpressure — slow reader skips lagged events

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
async fn post_raw(url: &str, body: &str) -> Result<(u16, String), String> {
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
async fn get_raw(
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

fn jsonrpc_request(id: serde_json::Value, method: &str, params: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

fn send_message_params() -> serde_json::Value {
    let p = make_send_params("fn batch_test() {}");
    serde_json::to_value(&p).unwrap()
}

// ── Batch JSON-RPC tests (61-66) ─────────────────────────────────────────────

/// Test 61: Single-element batch — a batch `[{...}]` with one SendMessage.
pub async fn test_batch_single_element(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let body = format!(
        "[{}]",
        jsonrpc_request(serde_json::json!(1), "SendMessage", send_message_params())
    );
    match post_raw(&ctx.analyzer_url, &body).await {
        Ok((status, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if arr.len() == 1 && status == 200 => {
                    let has_result = arr[0].get("result").is_some();
                    let has_id = arr[0].get("id") == Some(&serde_json::json!(1));
                    if has_result && has_id {
                        TestResult::pass(
                            "batch-single-element",
                            start.elapsed().as_millis(),
                            "1-element batch returned array[1]",
                        )
                    } else {
                        TestResult::fail(
                            "batch-single-element",
                            start.elapsed().as_millis(),
                            &format!(
                                "unexpected response: {}",
                                &resp_body[..resp_body.len().min(100)]
                            ),
                        )
                    }
                }
                Ok(arr) => TestResult::fail(
                    "batch-single-element",
                    start.elapsed().as_millis(),
                    &format!("status={status}, array len={}", arr.len()),
                ),
                Err(e) => TestResult::fail(
                    "batch-single-element",
                    start.elapsed().as_millis(),
                    &format!("not a JSON array: {e}"),
                ),
            }
        }
        Err(e) => TestResult::fail("batch-single-element", start.elapsed().as_millis(), &e),
    }
}

/// Test 62: Multi-request batch — SendMessage + GetTask (chained via task ID).
pub async fn test_batch_multi_request(ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    // First, send a single message to get a task ID.
    let single = jsonrpc_request(serde_json::json!(100), "SendMessage", send_message_params());
    let pre = post_raw(&ctx.analyzer_url, &single).await;
    let task_id = match pre {
        Ok((200, body)) => {
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            // SendMessageResponse::Task serializes as {"task": {"id": "...", ...}}
            v["result"]["task"]["id"]
                .as_str()
                .or_else(|| v["result"]["id"].as_str())
                .map(|s| s.to_owned())
        }
        _ => None,
    };
    let Some(task_id) = task_id else {
        return TestResult::fail(
            "batch-multi-request",
            start.elapsed().as_millis(),
            "could not create initial task",
        );
    };

    // Now batch: SendMessage + GetTask
    let batch = format!(
        "[{},{}]",
        jsonrpc_request(serde_json::json!(1), "SendMessage", send_message_params()),
        jsonrpc_request(
            serde_json::json!(2),
            "GetTask",
            serde_json::json!({ "id": task_id }),
        ),
    );
    match post_raw(&ctx.analyzer_url, &batch).await {
        Ok((200, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if arr.len() == 2 => {
                    let r1_ok =
                        arr[0].get("result").is_some() && arr[0]["id"] == serde_json::json!(1);
                    let r2_ok =
                        arr[1].get("result").is_some() && arr[1]["id"] == serde_json::json!(2);
                    if r1_ok && r2_ok {
                        TestResult::pass(
                            "batch-multi-request",
                            start.elapsed().as_millis(),
                            "SendMessage + GetTask in batch",
                        )
                    } else {
                        TestResult::fail(
                            "batch-multi-request",
                            start.elapsed().as_millis(),
                            &format!("r1={r1_ok}, r2={r2_ok}"),
                        )
                    }
                }
                Ok(arr) => TestResult::fail(
                    "batch-multi-request",
                    start.elapsed().as_millis(),
                    &format!("expected 2 responses, got {}", arr.len()),
                ),
                Err(e) => TestResult::fail(
                    "batch-multi-request",
                    start.elapsed().as_millis(),
                    &format!("parse error: {e}"),
                ),
            }
        }
        Ok((status, body)) => TestResult::fail(
            "batch-multi-request",
            start.elapsed().as_millis(),
            &format!("status={status}, body={}", &body[..body.len().min(80)]),
        ),
        Err(e) => TestResult::fail("batch-multi-request", start.elapsed().as_millis(), &e),
    }
}

/// Test 63: Empty batch `[]` returns a JSON-RPC parse error.
pub async fn test_batch_empty(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    match post_raw(&ctx.analyzer_url, "[]").await {
        Ok((status, resp_body)) => {
            let v: serde_json::Value = serde_json::from_str(&resp_body).unwrap_or_default();
            let is_error = v.get("error").is_some();
            let error_code = v["error"]["code"].as_i64().unwrap_or(0);
            // JSON-RPC parse error = -32700, or invalid request = -32600
            if is_error && (error_code == -32700 || error_code == -32600) {
                TestResult::pass(
                    "batch-empty",
                    start.elapsed().as_millis(),
                    &format!("status={status}, error code={error_code}"),
                )
            } else {
                TestResult::fail(
                    "batch-empty",
                    start.elapsed().as_millis(),
                    &format!(
                        "expected error, got: {}",
                        &resp_body[..resp_body.len().min(100)]
                    ),
                )
            }
        }
        Err(e) => TestResult::fail("batch-empty", start.elapsed().as_millis(), &e),
    }
}

/// Test 64: Batch with mixed valid and invalid requests.
pub async fn test_batch_mixed_valid_invalid(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let batch = format!(
        r#"[{},{{"jsonrpc":"2.0","invalid":true}}]"#,
        jsonrpc_request(serde_json::json!(1), "SendMessage", send_message_params()),
    );
    match post_raw(&ctx.analyzer_url, &batch).await {
        Ok((200, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if arr.len() == 2 => {
                    let first_ok = arr[0].get("result").is_some();
                    let second_err = arr[1].get("error").is_some();
                    if first_ok && second_err {
                        TestResult::pass(
                            "batch-mixed",
                            start.elapsed().as_millis(),
                            "valid→result, invalid→error",
                        )
                    } else {
                        TestResult::fail(
                            "batch-mixed",
                            start.elapsed().as_millis(),
                            &format!("first_ok={first_ok}, second_err={second_err}"),
                        )
                    }
                }
                Ok(arr) => TestResult::fail(
                    "batch-mixed",
                    start.elapsed().as_millis(),
                    &format!("expected 2, got {}", arr.len()),
                ),
                Err(e) => TestResult::fail(
                    "batch-mixed",
                    start.elapsed().as_millis(),
                    &format!("parse: {e}"),
                ),
            }
        }
        Ok((status, body)) => TestResult::fail(
            "batch-mixed",
            start.elapsed().as_millis(),
            &format!("status={status}: {}", &body[..body.len().min(80)]),
        ),
        Err(e) => TestResult::fail("batch-mixed", start.elapsed().as_millis(), &e),
    }
}

/// Test 65: Streaming method (SendStreamingMessage) in batch returns error.
pub async fn test_batch_streaming_rejected(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let batch = format!(
        "[{}]",
        jsonrpc_request(
            serde_json::json!(1),
            "SendStreamingMessage",
            send_message_params(),
        ),
    );
    match post_raw(&ctx.analyzer_url, &batch).await {
        Ok((200, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if !arr.is_empty() => {
                    let is_error = arr[0].get("error").is_some();
                    if is_error {
                        TestResult::pass(
                            "batch-streaming-rejected",
                            start.elapsed().as_millis(),
                            "streaming in batch → error",
                        )
                    } else {
                        TestResult::fail(
                            "batch-streaming-rejected",
                            start.elapsed().as_millis(),
                            "expected error for streaming in batch",
                        )
                    }
                }
                _ => TestResult::fail(
                    "batch-streaming-rejected",
                    start.elapsed().as_millis(),
                    &format!(
                        "unexpected response: {}",
                        &resp_body[..resp_body.len().min(80)]
                    ),
                ),
            }
        }
        Ok((status, _)) => TestResult::fail(
            "batch-streaming-rejected",
            start.elapsed().as_millis(),
            &format!("status={status}"),
        ),
        Err(e) => TestResult::fail("batch-streaming-rejected", start.elapsed().as_millis(), &e),
    }
}

/// Test 66: SubscribeToTask in batch returns error.
pub async fn test_batch_subscribe_rejected(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let batch = format!(
        "[{}]",
        jsonrpc_request(
            serde_json::json!(1),
            "SubscribeToTask",
            serde_json::json!({ "id": "nonexistent-task" }),
        ),
    );
    match post_raw(&ctx.analyzer_url, &batch).await {
        Ok((200, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if !arr.is_empty() => {
                    let is_error = arr[0].get("error").is_some();
                    if is_error {
                        TestResult::pass(
                            "batch-subscribe-rejected",
                            start.elapsed().as_millis(),
                            "subscribe in batch → error",
                        )
                    } else {
                        TestResult::fail(
                            "batch-subscribe-rejected",
                            start.elapsed().as_millis(),
                            "expected error for subscribe in batch",
                        )
                    }
                }
                _ => TestResult::fail(
                    "batch-subscribe-rejected",
                    start.elapsed().as_millis(),
                    &format!("unexpected: {}", &resp_body[..resp_body.len().min(80)]),
                ),
            }
        }
        Ok((status, _)) => TestResult::fail(
            "batch-subscribe-rejected",
            start.elapsed().as_millis(),
            &format!("status={status}"),
        ),
        Err(e) => TestResult::fail("batch-subscribe-rejected", start.elapsed().as_millis(), &e),
    }
}

// ── Real auth rejection (67) ────────────────────────────────────────────────

/// Interceptor that actually rejects requests without a valid bearer token.
struct RejectingAuthInterceptor {
    required_token: String,
}

impl a2a_protocol_server::interceptor::ServerInterceptor for RejectingAuthInterceptor {
    fn before<'a>(
        &'a self,
        ctx: &'a a2a_protocol_server::CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if ctx.caller_identity.as_deref() != Some(&self.required_token) {
                Err(A2aError::new(
                    ErrorCode::UnsupportedOperation,
                    "auth rejected",
                ))
            } else {
                Ok(())
            }
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a a2a_protocol_server::CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

/// Test 67: A server interceptor that truly rejects unauthorized requests.
pub async fn test_real_auth_rejection(ctx: &TestContext) -> TestResult {
    let _ = ctx; // Uses its own ephemeral server.
    let start = Instant::now();

    // Spin up a dedicated agent with a rejecting interceptor.
    let (listener, addr) = bind_listener().await;
    let url = format!("http://{addr}");
    let card = AgentCard {
        name: "AuthTestAgent".into(),
        description: "Agent that rejects unauthenticated requests".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.clone(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "auth-test".into(),
            name: "Auth Test".into(),
            description: "Tests auth rejection".into(),
            tags: vec![],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    };

    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
            .with_agent_card(card)
            .with_interceptor(RejectingAuthInterceptor {
                required_token: "super-secret".into(),
            })
            .build()
            .expect("build auth test handler"),
    );
    serve_jsonrpc(listener, handler);

    // Request without credentials → should be rejected.
    let body = jsonrpc_request(serde_json::json!(1), "SendMessage", send_message_params());
    match post_raw(&url, &body).await {
        Ok((_status, resp_body)) => {
            let v: serde_json::Value = serde_json::from_str(&resp_body).unwrap_or_default();
            let is_error = v.get("error").is_some();
            let msg = v["error"]["message"].as_str().unwrap_or("");
            if is_error && msg.contains("rejected") {
                TestResult::pass(
                    "real-auth-rejection",
                    start.elapsed().as_millis(),
                    "unauthenticated request rejected",
                )
            } else {
                TestResult::fail(
                    "real-auth-rejection",
                    start.elapsed().as_millis(),
                    &format!(
                        "expected rejection, got: {}",
                        &resp_body[..resp_body.len().min(100)]
                    ),
                )
            }
        }
        Err(e) => TestResult::fail("real-auth-rejection", start.elapsed().as_millis(), &e),
    }
}

// ── Extended agent card (68) ────────────────────────────────────────────────

/// Test 68: GetExtendedAgentCard via JSON-RPC returns the configured card.
pub async fn test_extended_agent_card(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let body = jsonrpc_request(
        serde_json::json!(1),
        "GetExtendedAgentCard",
        serde_json::json!({}),
    );
    match post_raw(&ctx.analyzer_url, &body).await {
        Ok((200, resp_body)) => {
            let v: serde_json::Value = serde_json::from_str(&resp_body).unwrap_or_default();
            let result = &v["result"];
            let name = result["name"].as_str().unwrap_or("");
            let has_skills = result
                .get("skills")
                .and_then(|s| s.as_array())
                .is_some_and(|a| !a.is_empty());
            if name == "Code Analyzer" && has_skills {
                TestResult::pass(
                    "extended-agent-card",
                    start.elapsed().as_millis(),
                    &format!("card name={name}, has_skills={has_skills}"),
                )
            } else {
                TestResult::fail(
                    "extended-agent-card",
                    start.elapsed().as_millis(),
                    &format!("name={name}, has_skills={has_skills}"),
                )
            }
        }
        Ok((status, body)) => TestResult::fail(
            "extended-agent-card",
            start.elapsed().as_millis(),
            &format!("status={status}: {}", &body[..body.len().min(80)]),
        ),
        Err(e) => TestResult::fail("extended-agent-card", start.elapsed().as_millis(), &e),
    }
}

// ── Dynamic agent cards (69) ────────────────────────────────────────────────

/// A producer that returns a card with an incrementing counter.
struct CountingProducer {
    counter: std::sync::atomic::AtomicU32,
}

impl AgentCardProducer for CountingProducer {
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
        Box::pin(async move {
            let n = self
                .counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(AgentCard {
                name: format!("DynamicAgent-v{n}"),
                description: "Dynamically generated agent card".into(),
                version: format!("1.0.{n}"),
                supported_interfaces: vec![AgentInterface {
                    url: "http://localhost:0".into(),
                    protocol_binding: "JSONRPC".into(),
                    protocol_version: "1.0.0".into(),
                    tenant: None,
                }],
                default_input_modes: vec!["text/plain".into()],
                default_output_modes: vec!["text/plain".into()],
                skills: vec![],
                capabilities: AgentCapabilities::none(),
                provider: None,
                icon_url: None,
                documentation_url: None,
                security_schemes: None,
                security_requirements: None,
                signatures: None,
            })
        })
    }
}

/// Test 69: Dynamic agent card handler produces fresh cards on each request.
pub async fn test_dynamic_agent_card(_ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    let producer = CountingProducer {
        counter: std::sync::atomic::AtomicU32::new(0),
    };
    let handler = DynamicAgentCardHandler::new(producer);

    // First request — should get version 0.
    let req1 = Request::builder()
        .method("GET")
        .uri("/")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let resp1 = handler.handle(&req1).await;
    let body1 = resp1.into_body().collect().await.unwrap().to_bytes();
    let card1: serde_json::Value = serde_json::from_slice(&body1).unwrap_or_default();

    // Second request — should get version 1 (dynamic!).
    let req2 = Request::builder()
        .method("GET")
        .uri("/")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let resp2 = handler.handle(&req2).await;
    let body2 = resp2.into_body().collect().await.unwrap().to_bytes();
    let card2: serde_json::Value = serde_json::from_slice(&body2).unwrap_or_default();

    let name1 = card1["name"].as_str().unwrap_or("");
    let name2 = card2["name"].as_str().unwrap_or("");

    if name1 == "DynamicAgent-v0" && name2 == "DynamicAgent-v1" {
        TestResult::pass(
            "dynamic-agent-card",
            start.elapsed().as_millis(),
            &format!("{name1} → {name2}"),
        )
    } else {
        TestResult::fail(
            "dynamic-agent-card",
            start.elapsed().as_millis(),
            &format!("got {name1} and {name2}"),
        )
    }
}

// ── Agent card HTTP caching (70) ────────────────────────────────────────────

/// Test 70: Agent card endpoint returns ETag; re-request with If-None-Match
/// yields 304 Not Modified.
pub async fn test_agent_card_caching(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let card_url = format!("{}/.well-known/agent.json", ctx.analyzer_url);

    // First request — get ETag.
    match get_raw(&card_url, &[]).await {
        Ok((200, headers, _body)) => {
            let etag = headers
                .iter()
                .find(|(k, _)| k == "etag")
                .map(|(_, v)| v.clone());
            let Some(etag) = etag else {
                return TestResult::fail(
                    "agent-card-caching",
                    start.elapsed().as_millis(),
                    "no ETag in response",
                );
            };

            // Second request with If-None-Match — should get 304.
            match get_raw(&card_url, &[("if-none-match", &etag)]).await {
                Ok((304, _, _)) => TestResult::pass(
                    "agent-card-caching",
                    start.elapsed().as_millis(),
                    &format!("304 with ETag={etag}"),
                ),
                Ok((status, _, _)) => TestResult::fail(
                    "agent-card-caching",
                    start.elapsed().as_millis(),
                    &format!("expected 304, got {status}"),
                ),
                Err(e) => TestResult::fail("agent-card-caching", start.elapsed().as_millis(), &e),
            }
        }
        Ok((status, _, _)) => TestResult::fail(
            "agent-card-caching",
            start.elapsed().as_millis(),
            &format!("card fetch status={status}"),
        ),
        Err(e) => TestResult::fail("agent-card-caching", start.elapsed().as_millis(), &e),
    }
}

// ── Backpressure / Lagged (71) ───────────────────────────────────────────────

/// Test 71: When event queue capacity is tiny, rapid events cause lagging.
/// The stream still completes — the slow reader silently skips missed events.
pub async fn test_backpressure_lagged(_ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    // Spin up an agent with capacity=2 (very small) to force lagging.
    let (listener, addr) = bind_listener().await;
    let url = format!("http://{addr}");

    let metrics = Arc::new(TeamMetrics::new("BackpressureTest"));
    let card = AgentCard {
        name: "BackpressureAgent".into(),
        description: "Agent with tiny event queue".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.clone(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "bp-test".into(),
            name: "Backpressure Test".into(),
            description: "Tests lagged events".into(),
            tags: vec![],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none().with_streaming(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    };

    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
            .with_agent_card(card)
            .with_interceptor(AuditInterceptor::new("BackpressureTest"))
            .with_metrics(MetricsForward(Arc::clone(&metrics)))
            .with_event_queue_capacity(2)
            .build()
            .expect("build backpressure handler"),
    );
    serve_jsonrpc(listener, handler);

    // Use the SDK client to send a streaming request.
    let client = a2a_protocol_client::ClientBuilder::new(&url)
        .build()
        .unwrap();
    match client
        .stream_message(make_send_params(
            "fn bp() { let x = 1; let y = 2; let z = 3; }",
        ))
        .await
    {
        Ok(mut stream) => {
            let mut event_count = 0;
            let mut saw_completed = false;
            while let Some(event) = stream.next().await {
                match event {
                    Ok(a2a_protocol_types::events::StreamResponse::StatusUpdate(ev)) => {
                        event_count += 1;
                        if ev.status.state == a2a_protocol_types::task::TaskState::Completed {
                            saw_completed = true;
                        }
                    }
                    Ok(_) => event_count += 1,
                    Err(_) => break,
                }
            }
            // With capacity=2, the reader may miss some events but should still
            // see the final Completed status (it's the last thing emitted).
            if saw_completed {
                TestResult::pass(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    &format!("{event_count} events received, completed=true"),
                )
            } else {
                TestResult::fail(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    &format!("{event_count} events, no Completed seen"),
                )
            }
        }
        Err(e) => TestResult::fail(
            "backpressure-lagged",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}
