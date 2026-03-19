// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests 51-60: WebSocket transport and multi-tenancy dogfood tests.
//!
//! Exercises the new WebSocket transport and tenant-scoped stores via
//! the real agent-team infrastructure.

use std::sync::Arc;
use std::time::Instant;

use crate::cards::code_analyzer_card;
use crate::executors::CodeAnalyzerExecutor;
use crate::helpers::make_send_params;
use crate::infrastructure::{bind_listener, serve_jsonrpc};
use crate::tests::{TestContext, TestResult};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::store::{TenantAwareInMemoryTaskStore, TenantContext};

// ── WebSocket tests (require `websocket` feature) ───────────────────────────

/// Test 51: WebSocket send message over the code analyzer agent.
#[cfg(feature = "websocket")]
pub async fn test_ws_send_message(_ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::dispatch::WebSocketDispatcher;
    use a2a_protocol_types::jsonrpc::{JsonRpcRequest, JsonRpcSuccessResponse};
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    let start = Instant::now();

    // Start a WebSocket server with the CodeAnalyzer executor.
    let handler = Arc::new(
        RequestHandlerBuilder::new(CodeAnalyzerExecutor)
            .with_agent_card(code_analyzer_card("ws://localhost"))
            .build()
            .expect("build ws handler"),
    );
    let dispatcher = Arc::new(WebSocketDispatcher::new(handler));
    let ws_addr = dispatcher
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("start WS server");

    // Connect via WebSocket.
    let (mut ws, _) = tokio_tungstenite::connect_async(&format!("ws://{ws_addr}"))
        .await
        .expect("ws connect");

    // Send a JSON-RPC request.
    let params = make_send_params("fn main() { println!(\"hello\"); }");
    let rpc_req = JsonRpcRequest::with_params(
        serde_json::json!("ws-test-51"),
        "SendMessage",
        serde_json::to_value(&params).unwrap(),
    );
    let json = serde_json::to_string(&rpc_req).unwrap();
    ws.send(WsMessage::Text(json)).await.unwrap();

    // Read response.
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .expect("ws timeout")
        .unwrap()
        .unwrap();
    let text = msg.into_text().unwrap();

    match serde_json::from_str::<JsonRpcSuccessResponse<serde_json::Value>>(&text) {
        Ok(resp) => {
            if resp.id == Some(serde_json::json!("ws-test-51")) {
                TestResult::pass("51-ws-send-message", start.elapsed().as_millis(), "OK")
            } else {
                TestResult::fail(
                    "51-ws-send-message",
                    start.elapsed().as_millis(),
                    "wrong id",
                )
            }
        }
        Err(e) => TestResult::fail(
            "51-ws-send-message",
            start.elapsed().as_millis(),
            &format!("parse error: {e}"),
        ),
    }
}

/// Test 52: WebSocket streaming.
#[cfg(feature = "websocket")]
pub async fn test_ws_streaming(_ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::dispatch::WebSocketDispatcher;
    use a2a_protocol_types::jsonrpc::JsonRpcRequest;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    let start = Instant::now();

    let handler = Arc::new(
        RequestHandlerBuilder::new(CodeAnalyzerExecutor)
            .with_agent_card(code_analyzer_card("ws://localhost"))
            .build()
            .expect("build ws handler"),
    );
    let dispatcher = Arc::new(WebSocketDispatcher::new(handler));
    let ws_addr = dispatcher
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("start WS server");

    let (mut ws, _) = tokio_tungstenite::connect_async(&format!("ws://{ws_addr}"))
        .await
        .expect("ws connect");

    let params = make_send_params("fn main() {}");
    let rpc_req = JsonRpcRequest::with_params(
        serde_json::json!("ws-stream-52"),
        "SendStreamingMessage",
        serde_json::to_value(&params).unwrap(),
    );
    ws.send(WsMessage::Text(serde_json::to_string(&rpc_req).unwrap()))
        .await
        .unwrap();

    // Collect frames.
    let mut frame_count = 0;
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let msg = ws.next().await.unwrap().unwrap();
            let text = msg.into_text().unwrap();
            frame_count += 1;
            if text.contains("stream_complete") {
                break;
            }
        }
    });

    match timeout.await {
        Ok(()) if frame_count >= 3 => TestResult::pass(
            "52-ws-streaming",
            start.elapsed().as_millis(),
            &format!("{frame_count} frames"),
        ),
        Ok(()) => TestResult::fail(
            "52-ws-streaming",
            start.elapsed().as_millis(),
            &format!("too few frames: {frame_count}"),
        ),
        Err(_) => TestResult::fail("52-ws-streaming", start.elapsed().as_millis(), "timeout"),
    }
}

// ── gRPC tests (require `grpc` feature) ─────────────────────────────────────

/// Test 56: gRPC send message over the code analyzer agent.
#[cfg(feature = "grpc")]
pub async fn test_grpc_send_message(ctx: &TestContext) -> TestResult {
    use a2a_protocol_client::transport::grpc::GrpcTransport;
    use a2a_protocol_types::responses::SendMessageResponse;

    let start = Instant::now();

    let transport = match GrpcTransport::connect(&ctx.grpc_analyzer_url).await {
        Ok(t) => t,
        Err(e) => {
            return TestResult::fail(
                "56-grpc-send-message",
                start.elapsed().as_millis(),
                &format!("connect failed: {e}"),
            );
        }
    };

    let client = a2a_protocol_client::ClientBuilder::new(&ctx.grpc_analyzer_url)
        .with_custom_transport(transport)
        .without_tls()
        .build()
        .expect("build gRPC client");

    let params = make_send_params("fn main() { println!(\"grpc!\"); }");
    match client.send_message(params).await {
        Ok(SendMessageResponse::Task(task)) => {
            if task.status.state.is_terminal() {
                TestResult::pass(
                    "56-grpc-send-message",
                    start.elapsed().as_millis(),
                    &format!("task {}", task.id),
                )
            } else {
                TestResult::fail(
                    "56-grpc-send-message",
                    start.elapsed().as_millis(),
                    &format!("non-terminal: {:?}", task.status.state),
                )
            }
        }
        Ok(SendMessageResponse::Message(_)) => TestResult::pass(
            "56-grpc-send-message",
            start.elapsed().as_millis(),
            "immediate message response",
        ),
        Ok(_) => TestResult::fail(
            "56-grpc-send-message",
            start.elapsed().as_millis(),
            "unexpected response variant",
        ),
        Err(e) => TestResult::fail(
            "56-grpc-send-message",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 57: gRPC streaming over the code analyzer agent.
#[cfg(feature = "grpc")]
pub async fn test_grpc_streaming(ctx: &TestContext) -> TestResult {
    use a2a_protocol_client::transport::grpc::GrpcTransport;

    let start = Instant::now();

    let transport = match GrpcTransport::connect(&ctx.grpc_analyzer_url).await {
        Ok(t) => t,
        Err(e) => {
            return TestResult::fail(
                "57-grpc-streaming",
                start.elapsed().as_millis(),
                &format!("connect failed: {e}"),
            );
        }
    };

    let client = a2a_protocol_client::ClientBuilder::new(&ctx.grpc_analyzer_url)
        .with_custom_transport(transport)
        .without_tls()
        .build()
        .expect("build gRPC client");

    let params = make_send_params("fn main() {}");
    match client.stream_message(params).await {
        Ok(mut stream) => {
            let mut event_count = 0u32;
            let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(_) => event_count += 1,
                        Err(e) => {
                            return Err(format!("stream error: {e}"));
                        }
                    }
                }
                Ok(event_count)
            });
            match timeout.await {
                Ok(Ok(count)) if count >= 2 => TestResult::pass(
                    "57-grpc-streaming",
                    start.elapsed().as_millis(),
                    &format!("{count} events"),
                ),
                Ok(Ok(count)) => TestResult::fail(
                    "57-grpc-streaming",
                    start.elapsed().as_millis(),
                    &format!("too few events: {count}"),
                ),
                Ok(Err(msg)) => {
                    TestResult::fail("57-grpc-streaming", start.elapsed().as_millis(), &msg)
                }
                Err(_) => {
                    TestResult::fail("57-grpc-streaming", start.elapsed().as_millis(), "timeout")
                }
            }
        }
        Err(e) => TestResult::fail(
            "57-grpc-streaming",
            start.elapsed().as_millis(),
            &format!("stream_message failed: {e}"),
        ),
    }
}

/// Test 58: gRPC get_task after send_message.
#[cfg(feature = "grpc")]
pub async fn test_grpc_get_task(ctx: &TestContext) -> TestResult {
    use a2a_protocol_client::transport::grpc::GrpcTransport;
    use a2a_protocol_types::responses::SendMessageResponse;

    let start = Instant::now();

    let transport = match GrpcTransport::connect(&ctx.grpc_analyzer_url).await {
        Ok(t) => t,
        Err(e) => {
            return TestResult::fail(
                "58-grpc-get-task",
                start.elapsed().as_millis(),
                &format!("connect failed: {e}"),
            );
        }
    };

    let client = a2a_protocol_client::ClientBuilder::new(&ctx.grpc_analyzer_url)
        .with_custom_transport(transport)
        .without_tls()
        .build()
        .expect("build gRPC client");

    // First send a message to create a task.
    let params = make_send_params("fn grpc_test() {}");
    let task_id = match client.send_message(params).await {
        Ok(SendMessageResponse::Task(task)) => task.id.to_string(),
        Ok(_) => {
            return TestResult::fail(
                "58-grpc-get-task",
                start.elapsed().as_millis(),
                "got non-task response",
            );
        }
        Err(e) => {
            return TestResult::fail(
                "58-grpc-get-task",
                start.elapsed().as_millis(),
                &format!("send failed: {e}"),
            );
        }
    };

    // Now get the task by ID.
    let query = a2a_protocol_types::params::TaskQueryParams {
        tenant: None,
        id: task_id.clone(),
        history_length: None,
    };
    match client.get_task(query).await {
        Ok(task) => {
            if task.id.to_string() == task_id {
                TestResult::pass(
                    "58-grpc-get-task",
                    start.elapsed().as_millis(),
                    &format!("task {task_id}"),
                )
            } else {
                TestResult::fail(
                    "58-grpc-get-task",
                    start.elapsed().as_millis(),
                    "wrong task ID returned",
                )
            }
        }
        Err(e) => TestResult::fail(
            "58-grpc-get-task",
            start.elapsed().as_millis(),
            &format!("get_task failed: {e}"),
        ),
    }
}

// ── Multi-tenancy tests ─────────────────────────────────────────────────────

/// Test 53: Tenant isolation — different tenants can't see each other's tasks.
pub async fn test_tenant_isolation(_ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    let store = Arc::new(TenantAwareInMemoryTaskStore::new());
    let (listener, addr) = bind_listener().await;
    let url = format!("http://{addr}");

    let handler = Arc::new(
        RequestHandlerBuilder::new(CodeAnalyzerExecutor)
            .with_agent_card(code_analyzer_card(&url))
            .with_task_store_arc(store.clone())
            .build()
            .expect("build tenant handler"),
    );
    serve_jsonrpc(listener, Arc::clone(&handler));

    // Send a message as tenant-alpha.
    let mut params_a = make_send_params("fn alpha() {}");
    params_a.tenant = Some("tenant-alpha".into());

    let client = a2a_protocol_client::ClientBuilder::new(&url)
        .without_tls()
        .build()
        .expect("build client");

    match client.send_message(params_a).await {
        Ok(a2a_protocol_types::responses::SendMessageResponse::Task(task)) => {
            // Now try to get this task as tenant-beta — should fail.
            let query = a2a_protocol_types::params::TaskQueryParams {
                tenant: Some("tenant-beta".into()),
                id: task.id.to_string(),
                history_length: None,
            };
            match client.get_task(query).await {
                Err(_) => TestResult::pass(
                    "53-tenant-isolation",
                    start.elapsed().as_millis(),
                    "correctly isolated",
                ),
                Ok(_) => TestResult::fail(
                    "53-tenant-isolation",
                    start.elapsed().as_millis(),
                    "tenant-beta saw tenant-alpha task",
                ),
            }
        }
        Ok(_) => TestResult::fail(
            "53-tenant-isolation",
            start.elapsed().as_millis(),
            "unexpected response type",
        ),
        Err(e) => TestResult::fail(
            "53-tenant-isolation",
            start.elapsed().as_millis(),
            &format!("send failed: {e}"),
        ),
    }
}

/// Test 54: Same task ID in different tenants doesn't collide.
pub async fn test_tenant_id_independence(_ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::store::TaskStore;
    use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

    let start = Instant::now();
    let store = Arc::new(TenantAwareInMemoryTaskStore::new());

    // Insert a task with the same ID under two different tenants.
    let task_a = Task {
        id: TaskId::new("shared-id"),
        context_id: a2a_protocol_types::task::ContextId::new("ctx-a"),
        status: TaskStatus::new(TaskState::Completed),
        history: None,
        artifacts: None,
        metadata: None,
    };
    let task_b = Task {
        id: TaskId::new("shared-id"),
        context_id: a2a_protocol_types::task::ContextId::new("ctx-b"),
        status: TaskStatus::new(TaskState::Working),
        history: None,
        artifacts: None,
        metadata: None,
    };

    TenantContext::scope("alpha".to_string(), {
        let store = store.clone();
        let task = task_a.clone();
        async move { store.save(task).await.unwrap() }
    })
    .await;

    TenantContext::scope("beta".to_string(), {
        let store = store.clone();
        let task = task_b.clone();
        async move { store.save(task).await.unwrap() }
    })
    .await;

    // Verify they have different context_ids (confirming independence).
    let alpha_task = TenantContext::scope("alpha".to_string(), {
        let store = store.clone();
        async move { store.get(&TaskId::new("shared-id")).await.unwrap().unwrap() }
    })
    .await;

    let beta_task = TenantContext::scope("beta".to_string(), {
        let store = store.clone();
        async move { store.get(&TaskId::new("shared-id")).await.unwrap().unwrap() }
    })
    .await;

    if alpha_task.context_id.as_ref() == "ctx-a" && beta_task.context_id.as_ref() == "ctx-b" {
        TestResult::pass(
            "54-tenant-id-independence",
            start.elapsed().as_millis(),
            "different contexts per tenant",
        )
    } else {
        TestResult::fail(
            "54-tenant-id-independence",
            start.elapsed().as_millis(),
            "contexts mixed up",
        )
    }
}

/// Test 55: Tenant count tracking.
pub async fn test_tenant_count(_ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::store::TaskStore;
    use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

    let start = Instant::now();
    let store = Arc::new(TenantAwareInMemoryTaskStore::new());

    for i in 0..5 {
        let store = store.clone();
        TenantContext::scope(format!("tenant-{i}"), async move {
            store
                .save(Task {
                    id: TaskId::new(format!("t-{i}")),
                    context_id: a2a_protocol_types::task::ContextId::new("ctx"),
                    status: TaskStatus::new(TaskState::Completed),
                    history: None,
                    artifacts: None,
                    metadata: None,
                })
                .await
                .unwrap();
        })
        .await;
    }

    let count = store.tenant_count().await;
    if count == 5 {
        TestResult::pass(
            "55-tenant-count",
            start.elapsed().as_millis(),
            &format!("{count} tenants"),
        )
    } else {
        TestResult::fail(
            "55-tenant-count",
            start.elapsed().as_millis(),
            &format!("expected 5, got {count}"),
        )
    }
}
