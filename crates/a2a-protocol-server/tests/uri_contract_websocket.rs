// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `WEBSOCKET_BINDING_URI`, pinned to the binding's wire contract as a live
//! server shows it.
//!
//! A2A v1.0.1 §5.8: "When a breaking change is introduced to a binding, a
//! new URI MUST be used." This SDK authors the WebSocket binding, so the
//! obligation is its own. The test drives a real `WebSocketDispatcher`,
//! records each frame the binding specification
//! (`book/src/bindings/websocket.md`) defines, with only the human-readable
//! `message` text redacted, and compares the document with
//! `tests/golden/uri_contracts/websocket.json`: the handshake refusal and
//! its body, the query-string fallback, the stream terminator, the refusals
//! of a binary frame, a batch and a v0.3 method name, an A2A error with its
//! details, notifications, and both credential refusals. A changed frame
//! with the URI unchanged fails, and so does a bumped URI with the old
//! snapshot. Re-bless with
//!
//! ```text
//! A2A_BLESS_URI_CONTRACTS=1 cargo test -p a2a-protocol-server --features websocket --test uri_contract_websocket
//! ```
//!
//! The method names, params and results are the JSON-RPC binding's, which
//! the specification defines; they are not this URI's to version and are
//! not recorded. Neither are the server's limits (message size, requests in
//! flight, timeouts), which a deployment tunes.
//!
//! The extensions' twin is the types crate's `tests/uri_contracts.rs`.

#![cfg(feature = "websocket")]

#[path = "uri_contract_websocket/golden.rs"]
mod golden;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::WebSocketDispatcher;
use a2a_protocol_server::interceptor::ServerInterceptor;
use a2a_protocol_server::{BearerTokenAuthInterceptor, CallContext, EventEmitter, agent_executor};
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::{TaskState, WEBSOCKET_BINDING_URI};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;

struct Done;
agent_executor!(Done, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Completed)
        .await
});

/// Refuses every call as a known caller without the permission.
struct Forbid;
impl ServerInterceptor for Forbid {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Err(A2aError::permission_denied("not allowed")) })
    }
    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn serve(builder: RequestHandlerBuilder) -> String {
    let handler = Arc::new(builder.without_agent_card().build().expect("a handler"));
    let addr = Arc::new(WebSocketDispatcher::new(handler))
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("a listener");
    format!("ws://{addr}/")
}

async fn connect(url: &str) -> Ws {
    let mut req = url.into_client_request().expect("a request");
    req.headers_mut()
        .insert("a2a-version", "1.0".parse().expect("a header value"));
    tokio_tungstenite::connect_async(req)
        .await
        .expect("the upgrade succeeds")
        .0
}

async fn next_json(ws: &mut Ws) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a frame within 5 s")
            .expect("the connection is open")
            .expect("a frame");
        if let WsMessage::Text(t) = frame {
            return serde_json::from_str(&t).expect("a JSON frame");
        }
    }
}

async fn send(ws: &mut Ws, frame: &Value) {
    ws.send(WsMessage::Text(frame.to_string().into()))
        .await
        .expect("sent");
}

async fn call(ws: &mut Ws, frame: Value) -> Value {
    send(ws, &frame).await;
    redact(next_json(ws).await)
}

/// `value` with the prose under every `message` key replaced: wording is
/// not contract, and pinning it would make every reworded error a "change".
fn redact(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    if k == "message" && v.is_string() {
                        (k, json!("<text>"))
                    } else {
                        (k, redact(v))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(redact).collect()),
        other => other,
    }
}

fn get_task(id: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "GetTask", "params": {"id": "task-9"}})
}

#[tokio::test]
async fn websocket_binding_contract() {
    let url = serve(RequestHandlerBuilder::new(Done)).await;

    // §1: the handshake. The query-string fallback for the version (§2).
    let refused = match tokio_tungstenite::connect_async(url.as_str()).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            let body: Value = serde_json::from_slice(resp.body().as_deref().unwrap_or_default())
                .expect("a JSON body");
            json!({ "status": resp.status().as_u16(), "body": redact(body) })
        }
        other => panic!("expected a refusal, got {:?}", other.map(|_| ())),
    };
    let fallback = tokio_tungstenite::connect_async(format!("{url}?A2A-Version=1.0"))
        .await
        .is_ok();

    // §3 and §5: refusals and an A2A error.
    let mut ws = connect(&url).await;
    ws.send(WsMessage::Binary(vec![1, 2, 3].into()))
        .await
        .expect("sent");
    let binary = redact(next_json(&mut ws).await);
    let batch = call(&mut ws, json!([get_task("b")])).await;
    let legacy = call(
        &mut ws,
        json!({"jsonrpc": "2.0", "id": "l", "method": "tasks/get", "params": {"id": "task-9"}}),
    )
    .await;
    let not_found = call(&mut ws, get_task("1")).await;

    // §3: a notification is answered by nothing. The probe after it is
    // answered; a frame for the notification would arrive before the
    // probe's or within the grace after it.
    send(
        &mut ws,
        &json!({"jsonrpc": "2.0", "method": "GetTask", "params": {"id": "task-9"}}),
    )
    .await;
    let probe = call(&mut ws, get_task("probe")).await;
    let extra = tokio::time::timeout(Duration::from_millis(300), ws.next()).await;
    let notification_answered = probe["id"] != "probe" || extra.is_ok();

    // §4: a stream's frames, then its terminator.
    send(
        &mut ws,
        &json!({"jsonrpc": "2.0", "id": "7", "method": "SendStreamingMessage", "params": {
            "message": {"messageId": "m-1", "role": "ROLE_USER", "parts": [{"text": "hi"}]}
        }}),
    )
    .await;
    // Which events a stream carries is the server's and the executor's
    // business; what a frame carrying one looks like is the binding's.
    let mut event_frames: Vec<Value> = Vec::new();
    let terminator = loop {
        let frame = next_json(&mut ws).await;
        let result = frame["result"].as_object().expect("a success frame");
        if result.get("status").and_then(Value::as_str) == Some("stream_complete") {
            break frame;
        }
        let kinds: Vec<&str> = result.keys().map(String::as_str).collect();
        assert!(
            matches!(
                kinds[..],
                ["task" | "message" | "statusUpdate" | "artifactUpdate"]
            ),
            "one StreamResponse per frame: {frame}"
        );
        let mut shape = frame.clone();
        shape["result"] =
            json!("<one StreamResponse: task | message | statusUpdate | artifactUpdate>");
        if !event_frames.contains(&shape) {
            event_frames.push(shape);
        }
    };

    // §6: the two credential refusals.
    let url = serve(
        RequestHandlerBuilder::new(Done)
            .with_interceptor(BearerTokenAuthInterceptor::new(["good"])),
    )
    .await;
    let unauthenticated = call(&mut connect(&url).await, get_task("1")).await;
    let url = serve(RequestHandlerBuilder::new(Done).with_interceptor(Forbid)).await;
    let permission_denied = call(&mut connect(&url).await, get_task("1")).await;

    golden::check(
        "websocket",
        WEBSOCKET_BINDING_URI,
        json!({
            "handshake": {
                "withoutVersion": refused,
                "versionQueryParameterAccepted": fallback,
            },
            "frames": {
                "binaryFrame": binary,
                "batch": batch,
                "v03MethodName": legacy,
                "a2aError": not_found,
                "notificationAnswered": notification_answered,
            },
            "stream": {
                "eventFrames": event_frames,
                "terminator": terminator,
            },
            "credentialRefusals": {
                "unauthenticated": unauthenticated,
                "permissionDenied": permission_denied,
            },
        }),
    );
}
