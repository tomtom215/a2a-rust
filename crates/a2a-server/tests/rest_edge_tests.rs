// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! REST dispatcher edge case tests: percent-encoded path traversal,
//! oversized query strings, boundary conditions.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::RestDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

struct EchoExecutor;

impl AgentExecutor for EchoExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::with_timestamp(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

fn make_handler() -> Arc<a2a_protocol_server::RequestHandler> {
    Arc::new(RequestHandlerBuilder::new(EchoExecutor).build().unwrap())
}

async fn start_rest_server(
    handler: Arc<a2a_protocol_server::RequestHandler>,
) -> std::net::SocketAddr {
    let dispatcher = Arc::new(RestDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let d = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&d);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });

    addr
}

async fn http_request(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
    content_type: Option<&str>,
) -> (u16, String) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();

    let mut builder = hyper::Request::builder()
        .method(method)
        .uri(format!("http://{addr}{path}"));

    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }

    let body_bytes = body.unwrap_or("").as_bytes().to_vec();
    let req = builder.body(Full::new(Bytes::from(body_bytes))).unwrap();

    let resp = client.request(req).await.unwrap();
    let status = resp.status().as_u16();
    let body = resp.collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

// ── Percent-encoded path traversal ──────────────────────────────────────────

#[tokio::test]
async fn percent_encoded_path_traversal_rejected() {
    let addr = start_rest_server(make_handler()).await;
    // %2E%2E is the percent-encoded form of ".."
    let (status, _) =
        http_request(addr, "GET", "/tasks/%2E%2E/%2E%2E/etc/passwd", None, None).await;
    assert_eq!(
        status, 400,
        "percent-encoded path traversal should be rejected"
    );
}

#[tokio::test]
async fn mixed_case_percent_encoded_traversal_rejected() {
    let addr = start_rest_server(make_handler()).await;
    // %2e%2e (lowercase hex) should also be caught.
    let (status, _) =
        http_request(addr, "GET", "/tasks/%2e%2e/%2e%2e/etc/shadow", None, None).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn double_dot_in_task_id_rejected() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "GET", "/tasks/../admin", None, None).await;
    assert_eq!(status, 400);
}

// ── Oversized query strings ─────────────────────────────────────────────────

#[tokio::test]
async fn oversized_query_string_rejected() {
    let addr = start_rest_server(make_handler()).await;
    // Create a query string that exceeds the 4 KiB limit.
    let long_query = format!("/tasks?q={}", "a".repeat(5000));
    let (status, _) = http_request(addr, "GET", &long_query, None, None).await;
    assert_eq!(status, 414, "oversized query string should return 414");
}

#[tokio::test]
async fn query_string_at_limit_accepted() {
    let addr = start_rest_server(make_handler()).await;
    // 4096 bytes of query is the limit — just at the boundary.
    let query = format!("/tasks?q={}", "a".repeat(4090));
    let (status, _) = http_request(addr, "GET", &query, None, None).await;
    // Should not be rejected (may be 200 or 404 depending on routing, but not 414).
    assert_ne!(status, 414, "query at limit should not be rejected");
}

// ── Tenant path edge cases ──────────────────────────────────────────────────

#[tokio::test]
async fn tenant_path_with_deep_nesting() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "GET", "/tenants/my-tenant/tasks", None, None).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn tenant_path_missing_trailing_slash() {
    let addr = start_rest_server(make_handler()).await;
    // /tenants/foo with no trailing path should be treated as not a tenant route.
    let (status, _) = http_request(addr, "GET", "/tenants/foo", None, None).await;
    assert_eq!(status, 404);
}

// ── A2A content type ────────────────────────────────────────────────────────

#[tokio::test]
async fn a2a_content_type_accepted() {
    let addr = start_rest_server(make_handler()).await;
    let body = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "ROLE_USER",
            "parts": [{"text": "hello"}]
        }
    });
    let (status, _) = http_request(
        addr,
        "POST",
        "/message:send",
        Some(&body.to_string()),
        Some("application/a2a+json"),
    )
    .await;
    // Content type should be accepted (not 415). The response may be 200 or 500
    // depending on whether the executor completes successfully.
    assert_ne!(
        status, 415,
        "application/a2a+json should be accepted as content type"
    );
}

// ── Cancel non-existent task ────────────────────────────────────────────────

#[tokio::test]
async fn cancel_nonexistent_task_returns_404() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "POST", "/tasks/nonexistent:cancel", None, None).await;
    assert_eq!(status, 404);
}

// ── Push config via REST ────────────────────────────────────────────────────

#[tokio::test]
async fn list_push_configs_for_nonexistent_task() {
    let addr = start_rest_server(make_handler()).await;
    let (status, body) = http_request(
        addr,
        "GET",
        "/tasks/nonexistent/pushNotificationConfigs",
        None,
        None,
    )
    .await;
    // Should return 200 with empty list (not 404 — configs are task-agnostic).
    assert_eq!(status, 200);
    // The body should be an empty array or contain an empty list.
    assert!(body.contains("[]") || body.contains("configs"));
}

// ── Agent card endpoint ─────────────────────────────────────────────────────

#[tokio::test]
async fn well_known_agent_card_without_card() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "GET", "/.well-known/agent.json", None, None).await;
    // No card configured on the handler, should return 404.
    assert_eq!(status, 404);
}

// ── Body size boundary ──────────────────────────────────────────────────────

#[tokio::test]
async fn request_body_just_under_limit_accepted() {
    let addr = start_rest_server(make_handler()).await;
    // Build a request body that is just under 4 MiB.
    // The body won't be valid JSON, but the size check happens before parsing.
    // Actually, we need valid Content-Type to get past the CT check,
    // and the body will fail JSON parsing with 400, not 413.
    let body = format!(
        r#"{{"message":{{"messageId":"m","role":"ROLE_USER","parts":[{{"text":"{}"}}]}}}}"#,
        "x".repeat(4 * 1024 * 1024 - 200)
    );
    let (status, _) = http_request(
        addr,
        "POST",
        "/message:send",
        Some(&body),
        Some("application/json"),
    )
    .await;
    // Should not be 413 (payload too large).
    assert_ne!(
        status, 413,
        "body just under limit should not be rejected as too large"
    );
}

// ── Double-encoded path traversal ──────────────────────────────────────────

/// Bug #35: Path traversal check must catch double-encoded sequences.
/// `%252E%252E` decodes to `%2E%2E` then to `..`.
#[tokio::test]
async fn double_encoded_path_traversal_rejected() {
    let addr = start_rest_server(make_handler()).await;
    // %252E = double-encoded '.', so %252E%252E/%252E%252E = ../../..
    let (status, _) =
        http_request(addr, "GET", "/%252E%252E/%252E%252E/etc/passwd", None, None).await;
    assert_eq!(
        status, 400,
        "double-encoded path traversal must be rejected"
    );
}

/// Single-encoded path traversal must also be rejected.
#[tokio::test]
async fn single_encoded_path_traversal_rejected() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "GET", "/%2E%2E/%2E%2E/etc/passwd", None, None).await;
    assert_eq!(
        status, 400,
        "single-encoded path traversal must be rejected"
    );
}

/// Raw path traversal must also be rejected.
#[tokio::test]
async fn raw_path_traversal_rejected() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "GET", "/../../etc/passwd", None, None).await;
    assert_eq!(status, 400, "raw path traversal must be rejected");
}
