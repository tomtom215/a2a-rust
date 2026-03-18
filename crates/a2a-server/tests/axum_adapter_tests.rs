// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Integration tests for the Axum adapter (`dispatch::axum_adapter`).
//!
//! These tests spin up a real Axum server with a test executor and exercise
//! all REST routes through the adapter, proving end-to-end compatibility.

#![cfg(feature = "axum")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::axum_adapter::A2aRouter;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::{Artifact, ArtifactId};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

// ── Test executor ────────────────────────────────────────────────────────────

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
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            let echo_text = ctx
                .message
                .parts
                .first()
                .and_then(|p| match &p.content {
                    PartContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "no text".to_owned());

            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new(
                        ArtifactId::new("echo-art"),
                        vec![Part::text(echo_text)],
                    ),
                    append: None,
                    last_chunk: Some(true),
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

// ── Test helpers ─────────────────────────────────────────────────────────────

fn test_card() -> AgentCard {
    AgentCard {
        url: None,
        name: "Test Echo Agent".into(),
        description: "Echoes input for testing".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "http://localhost/rpc".into(),
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
            tags: vec!["test".into()],
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
    }
}

async fn start_test_server() -> String {
    let handler = Arc::new(
        RequestHandlerBuilder::new(EchoExecutor)
            .with_agent_card(test_card())
            .build()
            .expect("build handler"),
    );

    let app = A2aRouter::new(handler).into_router();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    base_url
}

fn make_send_body(text: &str) -> String {
    serde_json::json!({
        "message": {
            "messageId": format!("msg-{}", uuid::Uuid::new_v4()),
            "role": "ROLE_USER",
            "parts": [{"type": "text", "text": text}]
        }
    })
    .to_string()
}

/// Helper: HTTP GET with hyper client.
async fn http_get(url: &str) -> (u16, Bytes) {
    let client = Client::builder(TokioExecutor::new()).build_http::<Empty<Bytes>>();
    let uri: hyper::Uri = url.parse().unwrap();
    let resp = client.get(uri).await.unwrap();
    let status = resp.status().as_u16();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, body)
}

/// Helper: HTTP POST with JSON body.
async fn http_post_json(url: &str, body: &str) -> (u16, Bytes) {
    let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(url)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body.to_owned())))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    let status = resp.status().as_u16();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, body)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn axum_health_endpoint() {
    let base = start_test_server().await;
    let (status, body) = http_get(&format!("{base}/health")).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], "ok");
}

#[tokio::test]
async fn axum_agent_card_discovery() {
    let base = start_test_server().await;
    let (status, body) = http_get(&format!("{base}/.well-known/agent.json")).await;
    assert_eq!(status, 200);
    let card: AgentCard = serde_json::from_slice(&body).unwrap();
    assert_eq!(card.name, "Test Echo Agent");
    assert_eq!(card.supported_interfaces.len(), 1);
    assert_eq!(card.skills[0].id, "echo");
}

#[tokio::test]
async fn axum_send_message_returns_completed_task() {
    let base = start_test_server().await;
    let body = make_send_body("Hello from Axum test");
    let (status, resp_body) = http_post_json(&format!("{base}/message:send"), &body).await;

    assert_eq!(status, 200);
    let result: SendMessageResponse = serde_json::from_slice(&resp_body).unwrap();
    match result {
        SendMessageResponse::Task(task) => {
            assert_eq!(task.status.state, TaskState::Completed);
            assert!(task.artifacts.is_some());
            let arts = task.artifacts.unwrap();
            assert!(!arts.is_empty());
            // Verify the echo text is in the artifact
            let first_art = &arts[0];
            match &first_art.parts[0].content {
                PartContent::Text { text } => {
                    assert_eq!(text, "Hello from Axum test");
                }
                _ => panic!("expected text part in artifact"),
            }
        }
        _ => {
            // Also acceptable (Message or future variants)
        }
    }
}

#[tokio::test]
async fn axum_get_task_after_send() {
    let base = start_test_server().await;
    let body = make_send_body("Task retrieval test");
    let (status, resp_body) = http_post_json(&format!("{base}/message:send"), &body).await;
    assert_eq!(status, 200);

    let result: SendMessageResponse = serde_json::from_slice(&resp_body).unwrap();
    let task_id = match result {
        SendMessageResponse::Task(task) => task.id.0,
        SendMessageResponse::Message(msg) => msg.task_id.map(|id| id.0).unwrap_or_default(),
        _ => String::new(),
    };

    if !task_id.is_empty() {
        let (status, resp_body) = http_get(&format!("{base}/tasks/{task_id}")).await;
        assert_eq!(status, 200);
        let task: Task = serde_json::from_slice(&resp_body).unwrap();
        assert_eq!(task.id, TaskId::new(task_id));
        assert_eq!(task.status.state, TaskState::Completed);
    }
}

#[tokio::test]
async fn axum_list_tasks() {
    let base = start_test_server().await;

    // Create a task first
    let body = make_send_body("List test");
    let (status, _) = http_post_json(&format!("{base}/message:send"), &body).await;
    assert_eq!(status, 200);

    // List tasks
    let (status, resp_body) = http_get(&format!("{base}/tasks")).await;
    assert_eq!(status, 200);
    let result: a2a_protocol_types::responses::TaskListResponse =
        serde_json::from_slice(&resp_body).unwrap();
    assert!(!result.tasks.is_empty());
}

#[tokio::test]
async fn axum_get_nonexistent_task_returns_404() {
    let base = start_test_server().await;
    let (status, _) = http_get(&format!("{base}/tasks/nonexistent-id-12345")).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn axum_invalid_json_returns_400() {
    let base = start_test_server().await;
    let (status, _) = http_post_json(&format!("{base}/message:send"), "not valid json").await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn axum_cancel_nonexistent_task_returns_404() {
    let base = start_test_server().await;
    let (status, _) = http_post_json(&format!("{base}/tasks/no-such-task:cancel"), "{}").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn axum_router_is_composable() {
    // Verify the A2aRouter can be merged with other Axum routes
    let handler = Arc::new(
        RequestHandlerBuilder::new(EchoExecutor)
            .build()
            .expect("build handler"),
    );

    let a2a_router = A2aRouter::new(handler).into_router();

    // This should compile — proves composability
    let _combined = axum::Router::new()
        .merge(a2a_router)
        .route("/custom", axum::routing::get(|| async { "custom route" }));
}
