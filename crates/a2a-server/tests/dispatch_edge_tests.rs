// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Edge case tests for REST and JSON-RPC dispatch layers via real HTTP.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};

use a2a_types::error::A2aResult;
use a2a_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_types::task::{TaskState, TaskStatus};

use a2a_server::builder::RequestHandlerBuilder;
use a2a_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_server::executor::AgentExecutor;
use a2a_server::request_context::RequestContext;
use a2a_server::streaming::EventQueueWriter;

struct EchoExecutor;

impl AgentExecutor for EchoExecutor {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::with_timestamp(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::with_timestamp(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

fn make_handler() -> Arc<a2a_server::RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(EchoExecutor)
            .build()
            .unwrap(),
    )
}

/// Start a server on a random port and return the address.
async fn start_rest_server(
    handler: Arc<a2a_server::RequestHandler>,
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

#[tokio::test]
async fn rest_health_check() {
    let addr = start_rest_server(make_handler()).await;
    let (status, body) = http_request(addr, "GET", "/health", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("ok"));
}

#[tokio::test]
async fn rest_ready_check() {
    let addr = start_rest_server(make_handler()).await;
    let (status, body) = http_request(addr, "GET", "/ready", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("ok"));
}

#[tokio::test]
async fn rest_not_found() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "GET", "/nonexistent", None, None).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn rest_path_traversal_rejected() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "GET", "/tasks/../../etc/passwd", None, None).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn rest_unsupported_content_type() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(
        addr,
        "POST",
        "/message:send",
        Some("not json"),
        Some("text/plain"),
    )
    .await;
    assert_eq!(status, 415);
}

#[tokio::test]
async fn rest_get_task_not_found() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "GET", "/tasks/nonexistent", None, None).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn rest_list_tasks_empty() {
    let addr = start_rest_server(make_handler()).await;
    let (status, body) = http_request(addr, "GET", "/tasks", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("tasks"));
}

#[tokio::test]
async fn rest_tenant_prefix_stripping() {
    let addr = start_rest_server(make_handler()).await;
    let (status, body) = http_request(
        addr,
        "GET",
        "/tenants/my-tenant/tasks",
        None,
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert!(body.contains("tasks"));
}

#[tokio::test]
async fn rest_extended_card_not_configured() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(addr, "GET", "/extendedAgentCard", None, None).await;
    assert_eq!(status, 500);
}

#[tokio::test]
async fn rest_send_message_bad_json() {
    let addr = start_rest_server(make_handler()).await;
    let (status, _) = http_request(
        addr,
        "POST",
        "/message:send",
        Some("not valid json"),
        Some("application/json"),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn rest_send_message_success() {
    let addr = start_rest_server(make_handler()).await;
    let body = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "ROLE_USER",
            "parts": [{"text": "hello"}]
        }
    });
    let (status, resp_body) = http_request(
        addr,
        "POST",
        "/message:send",
        Some(&body.to_string()),
        Some("application/json"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(resp_body.contains("TASK_STATE_COMPLETED"));
}

#[tokio::test]
async fn jsonrpc_unknown_method() {
    let handler = make_handler();
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let d = Arc::clone(&dispatcher);
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let io = hyper_util::rt::TokioIo::new(stream);
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

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "NonExistentMethod",
        "id": "req-1",
        "params": {}
    });

    let (status, resp_body) = http_request(
        addr,
        "POST",
        "/",
        Some(&body.to_string()),
        Some("application/json"),
    )
    .await;
    assert_eq!(status, 200); // JSON-RPC always returns 200
    assert!(resp_body.contains("Method not found"));
}

#[tokio::test]
async fn jsonrpc_invalid_json() {
    let handler = make_handler();
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let d = Arc::clone(&dispatcher);
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let io = hyper_util::rt::TokioIo::new(stream);
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

    let (status, resp_body) = http_request(
        addr,
        "POST",
        "/",
        Some("not valid json"),
        Some("application/json"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(resp_body.contains("Parse error"));
}

#[tokio::test]
async fn jsonrpc_unsupported_content_type() {
    let handler = make_handler();
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let d = Arc::clone(&dispatcher);
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let io = hyper_util::rt::TokioIo::new(stream);
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

    let (status, resp_body) = http_request(
        addr,
        "POST",
        "/",
        Some("{}"),
        Some("text/xml"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(resp_body.contains("Parse error"));
}
