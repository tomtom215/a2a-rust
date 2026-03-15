// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! End-to-end dispatch tests using a real TCP server.
//!
//! Starts a hyper server with both JSON-RPC and REST dispatchers and tests
//! request routing, response formats, and error handling.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};

use a2a_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use std::future::Future;
use std::pin::Pin;

use a2a_types::error::A2aResult;
use a2a_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_types::jsonrpc::{JsonRpcErrorResponse, JsonRpcRequest, JsonRpcSuccessResponse};
use a2a_types::message::{Message, MessageId, MessageRole, Part};
use a2a_types::params::MessageSendParams;
use a2a_types::push::TaskPushNotificationConfig;
use a2a_types::responses::SendMessageResponse;
use a2a_types::task::{Task, TaskState, TaskStatus};

use a2a_server::builder::RequestHandlerBuilder;
use a2a_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_server::executor::AgentExecutor;
use a2a_server::push::PushSender;
use a2a_server::request_context::RequestContext;
use a2a_server::streaming::EventQueueWriter;

// ── Test executor ───────────────────────────────────────────────────────────

struct SimpleExecutor;

impl AgentExecutor for SimpleExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Transition through Working before Completed (valid state machine).
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
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
        name: "Test Agent".into(),
        description: "A test agent".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "https://agent.example.com/rpc".into(),
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
        message: Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::text("hello")],
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

/// Start a JSON-RPC server on a random port and return the address.
async fn start_jsonrpc_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let handler = Arc::new(
        RequestHandlerBuilder::new(SimpleExecutor)
            .with_agent_card(minimal_agent_card())
            .with_push_sender(MockPushSender)
            .build()
            .expect("build handler"),
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let handle = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
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

    (addr, handle)
}

/// Start a REST server on a random port and return the address.
async fn start_rest_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let handler = Arc::new(
        RequestHandlerBuilder::new(SimpleExecutor)
            .with_agent_card(minimal_agent_card())
            .with_push_sender(MockPushSender)
            .build()
            .expect("build handler"),
    );
    let dispatcher = Arc::new(RestDispatcher::new(handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let handle = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
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

    (addr, handle)
}

/// Create a hyper client for testing.
fn http_client() -> hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::HttpConnector,
    Full<Bytes>,
> {
    hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new()).build_http()
}

// ── JSON-RPC dispatcher tests ───────────────────────────────────────────────

#[tokio::test]
async fn jsonrpc_send_message_returns_task() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(1),
        "SendMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<SendMessageResponse> =
        serde_json::from_slice(&body).expect("parse response");
    assert_eq!(result.id, Some(serde_json::json!(1)));
    match result.result {
        SendMessageResponse::Task(task) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Task variant"),
    }
}

#[tokio::test]
async fn jsonrpc_get_task_not_found() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(2),
        "GetTask",
        serde_json::json!({"id": "nonexistent"}),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    // JSON-RPC errors still return HTTP 200.
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    assert_eq!(result.id, Some(serde_json::json!(2)));
    // TaskNotFound code.
    assert!(result.error.code != 0, "expected non-zero error code");
}

#[tokio::test]
async fn jsonrpc_unknown_method() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc =
        JsonRpcRequest::with_params(serde_json::json!(3), "UnknownMethod", serde_json::json!({}));
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    // MethodNotFound = -32601
    assert_eq!(result.error.code, -32601);
}

#[tokio::test]
async fn jsonrpc_invalid_json() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from("not json at all")))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    // ParseError = -32700
    assert_eq!(result.error.code, -32700);
}

#[tokio::test]
async fn jsonrpc_missing_params() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    // GetTask without params.
    let rpc = JsonRpcRequest::new(serde_json::json!(4), "GetTask");
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    // InvalidParams = -32602
    assert_eq!(result.error.code, -32602);
}

#[tokio::test]
async fn jsonrpc_get_extended_agent_card() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::new(serde_json::json!(5), "GetExtendedAgentCard");
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<AgentCard> =
        serde_json::from_slice(&body).expect("parse response");
    assert_eq!(result.result.name, "Test Agent");
}

#[tokio::test]
async fn jsonrpc_list_tasks() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(serde_json::json!(6), "ListTasks", serde_json::json!({}));
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<serde_json::Value> =
        serde_json::from_slice(&body).expect("parse response");
    // Should be a valid response with tasks array.
    assert!(result.result.get("tasks").is_some());
}

#[tokio::test]
async fn jsonrpc_push_config_crud() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    // Create push config.
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(10),
        "CreateTaskPushNotificationConfig",
        serde_json::to_value(&config).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<TaskPushNotificationConfig> =
        serde_json::from_slice(&body).expect("parse response");
    assert!(result.result.id.is_some());
    let config_id = result.result.id.unwrap();

    // Get push config.
    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(11),
        "GetTaskPushNotificationConfig",
        serde_json::json!({"taskId": "task-1", "id": config_id}),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<TaskPushNotificationConfig> =
        serde_json::from_slice(&body).expect("parse get response");
    assert_eq!(result.result.url, "https://example.com/hook");

    // List push configs.
    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(12),
        "ListTaskPushNotificationConfigs",
        serde_json::json!({"id": "task-1"}),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<Vec<TaskPushNotificationConfig>> =
        serde_json::from_slice(&body).expect("parse list response");
    assert_eq!(result.result.len(), 1);

    // Delete push config.
    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(13),
        "DeleteTaskPushNotificationConfig",
        serde_json::json!({"taskId": "task-1", "id": config_id}),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn jsonrpc_send_streaming_returns_sse() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(20),
        "SendStreamingMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
}

// ── REST dispatcher tests ───────────────────────────────────────────────────

#[tokio::test]
async fn rest_send_message() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let body = serde_json::to_vec(&make_send_params()).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/message:send"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: SendMessageResponse = serde_json::from_slice(&body).expect("parse response");
    match result {
        SendMessageResponse::Task(task) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Task variant"),
    }
}

#[tokio::test]
async fn rest_send_streaming() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let body = serde_json::to_vec(&make_send_params()).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/message:stream"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
}

#[tokio::test]
async fn rest_get_task_not_found() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/tasks/nonexistent"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn rest_list_tasks() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/tasks"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert!(result.get("tasks").is_some());
}

#[tokio::test]
async fn rest_cancel_nonexistent_task() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/tasks/no-such-task:cancel"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn rest_subscribe_nonexistent_task() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/tasks/no-such-task:subscribe"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn rest_wellknown_agent_card() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/.well-known/agent.json"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/json")
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let card: AgentCard = serde_json::from_slice(&body).expect("parse card");
    assert_eq!(card.name, "Test Agent");
}

#[tokio::test]
async fn rest_extended_agent_card() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/extendedAgentCard"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let card: AgentCard = serde_json::from_slice(&body).expect("parse card");
    assert_eq!(card.name, "Test Agent");
}

#[tokio::test]
async fn rest_not_found_route() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/nonexistent/route"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn rest_push_config_crud() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    // Create push config.
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let body = serde_json::to_vec(&config).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!(
            "http://{addr}/tasks/task-1/pushNotificationConfigs"
        ))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let created: TaskPushNotificationConfig = serde_json::from_slice(&body).expect("parse config");
    assert!(created.id.is_some());
    let config_id = created.id.unwrap();

    // Get push config.
    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!(
            "http://{addr}/tasks/task-1/pushNotificationConfigs/{config_id}"
        ))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    // List push configs.
    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!(
            "http://{addr}/tasks/task-1/pushNotificationConfigs"
        ))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let configs: Vec<TaskPushNotificationConfig> =
        serde_json::from_slice(&body).expect("parse list");
    assert_eq!(configs.len(), 1);

    // Delete push config.
    let req = hyper::Request::builder()
        .method("DELETE")
        .uri(format!(
            "http://{addr}/tasks/task-1/pushNotificationConfigs/{config_id}"
        ))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn rest_push_config_not_supported_error_status() {
    // Build a handler without push sender.
    let handler = Arc::new(
        RequestHandlerBuilder::new(SimpleExecutor)
            .with_agent_card(minimal_agent_card())
            .build()
            .expect("build handler"),
    );
    let dispatcher = Arc::new(RestDispatcher::new(handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let io = hyper_util::rt::TokioIo::new(stream);
        let d = Arc::clone(&dispatcher);
        let service = hyper::service::service_fn(move |req| {
            let d = Arc::clone(&d);
            async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
        });
        let _ = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
            .serve_connection(io, service)
            .await;
    });

    let client = http_client();
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let body = serde_json::to_vec(&config).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!(
            "http://{addr}/tasks/task-1/pushNotificationConfigs"
        ))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    // PushNotSupported should map to 400 (bad request).
    assert_eq!(resp.status(), 400);
}

// ── REST full send + get roundtrip ──────────────────────────────────────────

#[tokio::test]
async fn rest_send_then_get_task() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    // Send a message.
    let body = serde_json::to_vec(&make_send_params()).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/message:send"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("send");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: SendMessageResponse = serde_json::from_slice(&body).expect("parse response");
    let task_id = match result {
        SendMessageResponse::Task(t) => t.id.0,
        _ => panic!("expected Task variant"),
    };

    // Get the task.
    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/tasks/{task_id}"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("get");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let fetched: Task = serde_json::from_slice(&body).expect("parse fetched task");
    assert_eq!(fetched.id.0, task_id);
    assert_eq!(fetched.status.state, TaskState::Completed);
}

// ── Phase 7 dispatch tests ─────────────────────────────────────────────────

#[tokio::test]
async fn rest_response_has_a2a_version_header() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let body = serde_json::to_vec(&make_send_params()).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/message:send"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("send");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("A2A-Version")
            .and_then(|v| v.to_str().ok()),
        Some("1.0.0"),
        "response should have A2A-Version: 1.0.0 header"
    );
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/a2a+json"),
        "response should have application/a2a+json content type"
    );
}

#[tokio::test]
async fn jsonrpc_response_has_a2a_version_header() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc_req = a2a_types::JsonRpcRequest::with_params(
        serde_json::json!(1),
        "SendMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc_req).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("send");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("A2A-Version")
            .and_then(|v| v.to_str().ok()),
        Some("1.0.0"),
    );
}

#[tokio::test]
async fn rest_tenant_prefix_routing() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    // Send a message via tenant-prefixed path.
    let body = serde_json::to_vec(&make_send_params()).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/tenants/acme/message:send"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("send via tenant prefix");
    assert_eq!(resp.status(), 200, "tenant-prefixed route should succeed");
}

#[tokio::test]
async fn rest_get_subscribe_allowed() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    // First create a task.
    let body = serde_json::to_vec(&make_send_params()).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/message:send"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("send");
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: SendMessageResponse = serde_json::from_slice(&body).expect("parse");
    let task_id = match result {
        SendMessageResponse::Task(t) => t.id.0,
        _ => panic!("expected Task"),
    };

    // GET /tasks/{id}:subscribe should be accepted (not 404).
    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/tasks/{task_id}:subscribe"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("subscribe via GET");
    // 200 for SSE or any non-404 status means the route matched.
    assert_ne!(
        resp.status(),
        404,
        "GET /tasks/:id:subscribe should be routed"
    );
}

// ── Hardening dispatch tests ────────────────────────────────────────────────

#[tokio::test]
async fn jsonrpc_rejects_wrong_content_type() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(1),
        "SendMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "text/plain")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    assert_eq!(
        result.error.code, -32700,
        "wrong content type should be ParseError"
    );
}

#[tokio::test]
async fn jsonrpc_accepts_a2a_content_type() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(1),
        "SendMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/a2a+json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert!(
        result.get("result").is_some(),
        "a2a+json should be accepted"
    );
}

#[tokio::test]
async fn rest_rejects_wrong_content_type_on_post() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let body = serde_json::to_vec(&make_send_params()).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/message:send"))
        .header("content-type", "text/xml")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 415, "wrong content type should return 415");
}

#[tokio::test]
async fn rest_health_endpoint_returns_ok() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/health"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse health");
    assert_eq!(value["status"], "ok");
}

#[tokio::test]
async fn rest_ready_endpoint_returns_ok() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/ready"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn rest_rejects_path_traversal() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/tasks/../../../etc/passwd"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 400, "path traversal should be rejected");
}
