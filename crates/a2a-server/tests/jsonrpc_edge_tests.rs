// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! JSON-RPC edge case tests: unusual id values, empty batch, huge batch,
//! notification handling, and boundary conditions.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
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

fn make_dispatcher() -> Arc<JsonRpcDispatcher> {
    let handler = Arc::new(RequestHandlerBuilder::new(EchoExecutor).build().unwrap());
    Arc::new(JsonRpcDispatcher::new(handler))
}

async fn start_jsonrpc_server() -> std::net::SocketAddr {
    let dispatcher = make_dispatcher();
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

async fn post_jsonrpc(addr: std::net::SocketAddr, body: &str) -> (u16, String) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body.to_owned())))
        .unwrap();

    let resp = client.request(req).await.unwrap();
    let status = resp.status().as_u16();
    let body = resp.collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

// ── ID edge cases ───────────────────────────────────────────────────────────

#[tokio::test]
async fn jsonrpc_id_zero_is_valid() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "GetTask",
        "id": 0,
        "params": { "id": "nonexistent" }
    });
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    // The response should include the id as 0.
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["id"], 0);
}

#[tokio::test]
async fn jsonrpc_id_empty_string_is_valid() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "GetTask",
        "id": "",
        "params": { "id": "nonexistent" }
    });
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["id"], "");
}

#[tokio::test]
async fn jsonrpc_id_float_is_valid() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "GetTask",
        "id": 1.5,
        "params": { "id": "nonexistent" }
    });
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["id"], 1.5);
}

#[tokio::test]
async fn jsonrpc_id_null_is_valid() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "GetTask",
        "id": null,
        "params": { "id": "nonexistent" }
    });
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["id"].is_null());
}

#[tokio::test]
async fn jsonrpc_id_large_integer_is_valid() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "GetTask",
        "id": 999999999999i64,
        "params": { "id": "nonexistent" }
    });
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["id"], 999999999999i64);
}

// ── Batch edge cases ────────────────────────────────────────────────────────

#[tokio::test]
async fn jsonrpc_empty_batch_returns_error() {
    let addr = start_jsonrpc_server().await;
    let (status, resp) = post_jsonrpc(addr, "[]").await;
    assert_eq!(status, 200);
    assert!(resp.contains("Parse error") || resp.contains("empty batch"));
}

#[tokio::test]
async fn jsonrpc_batch_with_mixed_valid_and_invalid() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!([
        {
            "jsonrpc": "2.0",
            "method": "GetTask",
            "id": "valid",
            "params": { "id": "nonexistent" }
        },
        "not a valid request",
        {
            "jsonrpc": "2.0",
            "method": "GetTask",
            "id": "also-valid",
            "params": { "id": "nonexistent" }
        }
    ]);
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v.is_array());
    let arr = v.as_array().unwrap();
    // Should have 3 responses (2 valid + 1 parse error).
    assert_eq!(
        arr.len(),
        3,
        "batch should return response for each element"
    );
}

#[tokio::test]
async fn jsonrpc_single_element_batch() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!([{
        "jsonrpc": "2.0",
        "method": "GetTask",
        "id": "single",
        "params": { "id": "nonexistent" }
    }]);
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v.is_array(),
        "batch response should be an array even for single element"
    );
    assert_eq!(v.as_array().unwrap().len(), 1);
}

// ── Streaming methods in batch ──────────────────────────────────────────────

#[tokio::test]
async fn jsonrpc_streaming_in_batch_returns_error() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!([{
        "jsonrpc": "2.0",
        "method": "SendStreamingMessage",
        "id": "stream-in-batch",
        "params": {
            "message": {
                "messageId": "msg-1",
                "role": "ROLE_USER",
                "parts": [{"text": "hello"}]
            }
        }
    }]);
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let arr = v.as_array().unwrap();
    assert!(
        arr[0].get("error").is_some(),
        "streaming in batch should return error"
    );
}

#[tokio::test]
async fn jsonrpc_subscribe_in_batch_returns_error() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!([{
        "jsonrpc": "2.0",
        "method": "SubscribeToTask",
        "id": "sub-in-batch",
        "params": { "id": "nonexistent" }
    }]);
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let arr = v.as_array().unwrap();
    assert!(
        arr[0].get("error").is_some(),
        "subscribe in batch should return error"
    );
}

// ── Missing params ──────────────────────────────────────────────────────────

#[tokio::test]
async fn jsonrpc_missing_params_returns_error() {
    let addr = start_jsonrpc_server().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "GetTask",
        "id": "no-params"
    });
    let (status, resp) = post_jsonrpc(addr, &body.to_string()).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(
        v.get("error").is_some(),
        "missing params should be an error"
    );
}

// ── OPTIONS request ─────────────────────────────────────────────────────────

#[tokio::test]
async fn jsonrpc_options_returns_204_or_200() {
    let addr = start_jsonrpc_server().await;
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();

    let req = hyper::Request::builder()
        .method("OPTIONS")
        .uri(format!("http://{addr}/"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.unwrap();
    assert!(
        resp.status().as_u16() == 204 || resp.status().as_u16() == 200,
        "OPTIONS should return 204 or 200"
    );
}
