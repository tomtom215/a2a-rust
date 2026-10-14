// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! JSON-RPC 2.0 notifications over HTTP (JSON-RPC 2.0 §4.1, audit register
//! R21): a Request object with no `id` member is run and never answered,
//! alone, in a batch, or refused on its headers. Until 2026-10-14 each was
//! answered with `"id": null`.

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

struct CompletingExecutor;

impl AgentExecutor for CompletingExecutor {
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

async fn start_server() -> std::net::SocketAddr {
    let handler = Arc::new(
        RequestHandlerBuilder::new(CompletingExecutor)
            .without_agent_card()
            .build()
            .unwrap(),
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
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

async fn post(
    addr: std::net::SocketAddr,
    version: &str,
    body: &serde_json::Value,
) -> (u16, String) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .header("a2a-version", version)
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    let status = resp.status().as_u16();
    let body = resp.collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn send_message(method: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": {
            "message": {
                "messageId": "m-1",
                "role": "ROLE_USER",
                "parts": [{"text": "hello"}]
            }
        }
    })
}

fn get_task(id: Option<&str>) -> serde_json::Value {
    let mut req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "GetTask",
        "params": {"id": "no-such-task"}
    });
    if let Some(id) = id {
        req["id"] = serde_json::json!(id);
    }
    req
}

async fn task_count(addr: std::net::SocketAddr) -> usize {
    let list =
        serde_json::json!({"jsonrpc": "2.0", "id": "list", "method": "ListTasks", "params": {}});
    let (status, body) = post(addr, "1.0", &list).await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    v["result"]["tasks"].as_array().map_or(0, Vec::len)
}

#[tokio::test]
async fn a_notification_is_run_and_answered_with_no_content() {
    let addr = start_server().await;
    let (status, body) = post(addr, "1.0", &send_message("SendMessage")).await;
    assert_eq!(status, 204);
    assert!(body.is_empty(), "a notification gets no body: {body}");
    assert_eq!(task_count(addr).await, 1, "the notification was run");
}

/// A notification that fails is not answered either: its sender asked for
/// no reply, error or not.
#[tokio::test]
async fn a_failing_notification_is_not_answered() {
    let addr = start_server().await;
    let (status, body) = post(addr, "1.0", &get_task(None)).await;
    assert_eq!(status, 204);
    assert!(body.is_empty(), "{body}");
}

/// A stream is the only answer a streaming method gives, so one sent as a
/// notification is refused rather than run, as in a batch.
#[tokio::test]
async fn a_streaming_notification_is_not_run() {
    let addr = start_server().await;
    let (status, body) = post(addr, "1.0", &send_message("SendStreamingMessage")).await;
    assert_eq!(status, 204);
    assert!(body.is_empty(), "{body}");
    assert_eq!(task_count(addr).await, 0);
}

#[tokio::test]
async fn a_batch_answers_its_calls_and_not_its_notifications() {
    let addr = start_server().await;
    let batch = serde_json::json!([get_task(None), get_task(Some("call")), get_task(None)]);
    let (status, body) = post(addr, "1.0", &batch).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let answers = v.as_array().expect("an array");
    assert_eq!(answers.len(), 1, "one answer, for the call: {body}");
    assert_eq!(answers[0]["id"], "call");
}

/// "If there are no Response objects contained within the Response array as
/// it is to be sent to the client, the server MUST NOT return an empty
/// Array and should return nothing at all" (JSON-RPC 2.0 §6).
#[tokio::test]
async fn a_batch_of_notifications_gets_no_content() {
    let addr = start_server().await;
    let batch = serde_json::json!([get_task(None), send_message("SendMessage")]);
    let (status, body) = post(addr, "1.0", &batch).await;
    assert_eq!(status, 204);
    assert!(body.is_empty(), "{body}");
    assert_eq!(task_count(addr).await, 1, "the batch's send was run");
}

/// A notification refused on its headers (here an unsupported `A2A-Version`)
/// is refused without an answer; a call in the same batch keeps its error.
#[tokio::test]
async fn a_notification_refused_on_its_headers_is_not_answered() {
    let addr = start_server().await;
    let (status, body) = post(addr, "0.3", &get_task(None)).await;
    assert_eq!(status, 204);
    assert!(body.is_empty(), "{body}");

    let batch = serde_json::json!([get_task(None), get_task(Some("call"))]);
    let (status, body) = post(addr, "0.3", &batch).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let answers = v.as_array().expect("an array");
    assert_eq!(answers.len(), 1, "{body}");
    assert_eq!(answers[0]["id"], "call");
    assert!(answers[0]["error"].is_object(), "{body}");
}

/// An object with no `id` that is not a valid Request is not a notification:
/// it is Invalid Request, answered with a null id (JSON-RPC 2.0 §5).
#[tokio::test]
async fn an_invalid_request_without_an_id_is_still_answered() {
    let addr = start_server().await;
    let (status, body) = post(addr, "1.0", &serde_json::json!({"jsonrpc": "2.0"})).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["code"], -32600, "{body}");
    assert!(v["id"].is_null());
}
