// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! End-to-end WebSocket transport tests.
//!
//! Starts a WebSocket server and connects via `tokio-tungstenite` to test
//! JSON-RPC message exchange over WebSocket frames.

#![cfg(feature = "websocket")]

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::jsonrpc::{JsonRpcErrorResponse, JsonRpcRequest, JsonRpcSuccessResponse};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::WebSocketDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::push::PushSender;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

// ── Test executor ───────────────────────────────────────────────────────────

struct SimpleExecutor;

impl AgentExecutor for SimpleExecutor {
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
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

struct MockPushSender;

impl PushSender for MockPushSender {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn minimal_agent_card() -> AgentCard {
    AgentCard {
        url: None,
        name: "WS Test Agent".into(),
        description: "A WebSocket test agent".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "ws://localhost/rpc".into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "echo".into(),
            name: "Echo".into(),
            description: "Echoes input".into(),
            tags: vec!["echo".into()],
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
    }
}

fn make_send_params() -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        context_id: None,
        message: Message {
            id: MessageId::new("msg-ws-1"),
            role: MessageRole::User,
            parts: vec![Part::text("hello via websocket")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }
}

async fn start_ws_server() -> std::net::SocketAddr {
    let handler = Arc::new(
        RequestHandlerBuilder::new(SimpleExecutor)
            .with_agent_card(minimal_agent_card())
            .with_push_sender(MockPushSender)
            .build()
            .expect("build handler"),
    );
    let dispatcher = Arc::new(WebSocketDispatcher::new(handler));
    dispatcher
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("start WS server")
}

async fn connect(
    addr: std::net::SocketAddr,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://{addr}");
    let (ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("WebSocket connect");
    ws
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn ws_send_message_returns_task() {
    let addr = start_ws_server().await;
    let mut ws = connect(addr).await;

    let rpc_req = JsonRpcRequest::with_params(
        serde_json::json!("req-1"),
        "SendMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let json = serde_json::to_string(&rpc_req).unwrap();
    ws.send(WsMessage::Text(json)).await.unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    let resp: JsonRpcSuccessResponse<serde_json::Value> = serde_json::from_str(&text)
        .unwrap_or_else(|_| panic!("should be success response, got: {text}"));
    assert_eq!(resp.id, Some(serde_json::json!("req-1")));
    // The result should be a Task or a Message response with a valid structure.
    let result_str = serde_json::to_string(&resp.result).unwrap();
    assert!(
        result_str.contains("completed")
            || result_str.contains("working")
            || result_str.contains("id"),
        "unexpected result: {result_str}"
    );
}

#[tokio::test]
async fn ws_streaming_sends_multiple_frames() {
    let addr = start_ws_server().await;
    let mut ws = connect(addr).await;

    let rpc_req = JsonRpcRequest::with_params(
        serde_json::json!("stream-1"),
        "SendStreamingMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let json = serde_json::to_string(&rpc_req).unwrap();
    ws.send(WsMessage::Text(json)).await.unwrap();

    // Collect frames until we get the final stream_complete response.
    let mut frames = Vec::new();
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = ws.next().await.unwrap().unwrap();
            let text = msg.into_text().unwrap();
            let is_complete = text.contains("stream_complete");
            frames.push(text);
            if is_complete {
                break;
            }
        }
    });
    timeout.await.expect("streaming should complete within 5s");

    // Should have at least the working + completed events + final response.
    assert!(
        frames.len() >= 3,
        "expected at least 3 frames, got {}",
        frames.len()
    );
}

#[tokio::test]
async fn ws_unknown_method_returns_error() {
    let addr = start_ws_server().await;
    let mut ws = connect(addr).await;

    let rpc_req = JsonRpcRequest::with_params(
        serde_json::json!("bad-1"),
        "NonExistentMethod",
        serde_json::Value::Null,
    );
    let json = serde_json::to_string(&rpc_req).unwrap();
    ws.send(WsMessage::Text(json)).await.unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    let resp: JsonRpcErrorResponse = serde_json::from_str(&text).expect("should be error response");
    assert!(resp.error.code != 0, "should have non-zero error code");
}

#[tokio::test]
async fn ws_malformed_json_returns_parse_error() {
    let addr = start_ws_server().await;
    let mut ws = connect(addr).await;

    ws.send(WsMessage::Text("not valid json{{{".into()))
        .await
        .unwrap();

    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    let resp: JsonRpcErrorResponse = serde_json::from_str(&text).expect("should be error response");
    assert_eq!(resp.error.code, -32700, "should be parse error");
}

#[tokio::test]
async fn ws_ping_pong() {
    let addr = start_ws_server().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
        .await
        .unwrap();

    ws.send(WsMessage::Ping(vec![1, 2, 3])).await.unwrap();

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let msg = ws.next().await.unwrap().unwrap();
            if let WsMessage::Pong(data) = msg {
                assert_eq!(&data[..], &[1, 2, 3]);
                return;
            }
        }
    });
    timeout.await.expect("should receive pong within 2s");
}

#[tokio::test]
async fn ws_multiple_requests_on_same_connection() {
    let addr = start_ws_server().await;
    let mut ws = connect(addr).await;

    for i in 0..3 {
        let rpc_req = JsonRpcRequest::with_params(
            serde_json::json!(format!("multi-{i}")),
            "SendMessage",
            serde_json::to_value(make_send_params()).unwrap(),
        );
        let json = serde_json::to_string(&rpc_req).unwrap();
        ws.send(WsMessage::Text(json)).await.unwrap();
    }

    // Collect 3 responses (may arrive in any order).
    let mut responses = Vec::new();
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for _ in 0..3 {
            let msg = ws.next().await.unwrap().unwrap();
            let text = msg.into_text().unwrap();
            responses.push(text);
        }
    });
    timeout
        .await
        .expect("should receive all 3 responses within 5s");
    assert_eq!(responses.len(), 3);
}

#[tokio::test]
async fn ws_close_frame_terminates_cleanly() {
    let addr = start_ws_server().await;
    let mut ws = connect(addr).await;

    ws.send(WsMessage::Close(None)).await.unwrap();

    // After close, next read should return None or Close.
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                Some(Ok(WsMessage::Close(_))) | None => return,
                Some(Ok(_)) => continue,
                Some(Err(_)) => return,
            }
        }
    });
    timeout.await.expect("should terminate cleanly within 2s");
}
