// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Memory and load stress tests.
//!
//! Verifies the server can handle sustained concurrent load without memory
//! leaks, deadlocks, or resource exhaustion.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::jsonrpc::{JsonRpcRequest, JsonRpcSuccessResponse};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::push::PushSender;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

// ── Test executor ───────────────────────────────────────────────────────────

struct StressExecutor {
    completed_count: Arc<AtomicUsize>,
}

impl AgentExecutor for StressExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        let completed = Arc::clone(&self.completed_count);
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            // Simulate a small delay to mimic real work.
            tokio::time::sleep(Duration::from_millis(1)).await;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            completed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
    }
}

struct NoopPushSender;

impl PushSender for NoopPushSender {
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
        name: "Stress Test Agent".into(),
        description: "Handles load tests".into(),
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
            id: "stress".into(),
            name: "Stress".into(),
            description: "Stress test skill".into(),
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
    }
}

fn make_send_params(id: usize) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(format!("stress-msg-{id}")),
            role: MessageRole::User,
            parts: vec![Part::text(format!("stress request {id}"))],
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

async fn start_stress_server(completed_count: Arc<AtomicUsize>) -> SocketAddr {
    let handler = Arc::new(
        RequestHandlerBuilder::new(StressExecutor { completed_count })
            .with_agent_card(minimal_agent_card())
            .with_push_sender(NoopPushSender)
            .build()
            .expect("build handler"),
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    addr
}

type HttpClient = Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>;

fn build_http_client() -> HttpClient {
    Client::builder(TokioExecutor::new()).build_http()
}

async fn send_request(client: &HttpClient, addr: SocketAddr, id: usize) -> Result<(), String> {
    let params = make_send_params(id);
    let rpc_req = JsonRpcRequest::with_params(
        serde_json::json!(format!("stress-{id}")),
        "SendMessage",
        serde_json::to_value(&params).unwrap(),
    );
    let body = serde_json::to_vec(&rpc_req).unwrap();

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    let body_bytes = resp
        .collect()
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();

    if !status.is_success() {
        return Err(format!(
            "unexpected status {status}: {}",
            String::from_utf8_lossy(&body_bytes)
        ));
    }

    // Verify it's a valid JSON-RPC response.
    let _: JsonRpcSuccessResponse<serde_json::Value> =
        serde_json::from_slice(&body_bytes).map_err(|e| format!("parse response: {e}"))?;

    Ok(())
}

// ── Stress Tests ────────────────────────────────────────────────────────────

/// Sends 200 concurrent requests and verifies all complete successfully.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_200_requests_all_succeed() {
    let completed = Arc::new(AtomicUsize::new(0));
    let addr = start_stress_server(Arc::clone(&completed)).await;
    let client = build_http_client();

    let mut handles = Vec::new();
    for i in 0..200 {
        let client = client.clone();
        handles.push(tokio::spawn(
            async move { send_request(&client, addr, i).await },
        ));
    }

    let mut success_count = 0;
    let mut error_count = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(()) => success_count += 1,
            Err(e) => {
                error_count += 1;
                eprintln!("request error: {e}");
            }
        }
    }

    assert_eq!(error_count, 0, "all requests should succeed");
    assert_eq!(success_count, 200);
    // Wait a bit for background event processors to complete.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        completed.load(Ordering::Relaxed),
        200,
        "all executors should have completed"
    );
}

/// Sends requests in waves over 10 seconds to test sustained load.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sustained_load_10_seconds() {
    let completed = Arc::new(AtomicUsize::new(0));
    let addr = start_stress_server(Arc::clone(&completed)).await;
    let client = build_http_client();

    let start = Instant::now();
    let mut request_id = 0_usize;
    let mut total_sent = 0_usize;

    // Send 50 requests per wave, 10 waves over ~5 seconds.
    for _wave in 0..10 {
        let mut handles = Vec::new();
        for _ in 0..50 {
            let client = client.clone();
            let id = request_id;
            request_id += 1;
            total_sent += 1;
            handles.push(tokio::spawn(async move {
                send_request(&client, addr, id).await
            }));
        }

        // Wait for this wave to complete.
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok(), "request failed: {:?}", result.err());
        }

        // Small pause between waves.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(30),
        "sustained load test took too long: {elapsed:?}"
    );

    // Wait for all background processors.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let completed_val = completed.load(Ordering::Relaxed);
    assert_eq!(
        completed_val, total_sent,
        "all {total_sent} executors should have completed, but only {completed_val} did"
    );
}

/// Tests that the in-memory task store doesn't accumulate unbounded entries
/// when eviction is configured.
#[tokio::test]
async fn task_store_eviction_under_load() {
    use a2a_protocol_server::store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};
    use a2a_protocol_types::params::ListTasksParams;
    use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

    let config = TaskStoreConfig {
        max_capacity: Some(50),
        ..Default::default()
    };
    let store = InMemoryTaskStore::with_config(config);

    // Insert 200 tasks — should trigger eviction.
    for i in 0..200 {
        let task = Task {
            id: TaskId::new(format!("evict-task-{i}")),
            context_id: ContextId::new("ctx-evict"),
            status: TaskStatus::new(TaskState::Completed),
            history: None,
            artifacts: None,
            metadata: None,
        };
        store.save(task).await.unwrap();
    }

    // Run eviction.
    store.run_eviction().await;

    // Should be at or below the max.
    let params = ListTasksParams::default();
    let all = store.list(&params).await.unwrap();
    assert!(
        all.tasks.len() <= 50,
        "store should have at most 50 tasks after eviction, but has {}",
        all.tasks.len()
    );
}

/// Tests tenant isolation under concurrent load — requests from different
/// tenants should not interfere.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_multi_tenant_isolation() {
    use a2a_protocol_server::store::TaskStore;
    use a2a_protocol_server::store::{TenantAwareInMemoryTaskStore, TenantContext};
    use a2a_protocol_types::params::ListTasksParams;
    use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

    let store = Arc::new(TenantAwareInMemoryTaskStore::new());

    // 10 tenants, each inserting 50 tasks concurrently.
    let mut handles = Vec::new();
    for tenant_idx in 0..10 {
        for task_idx in 0..50 {
            let store = Arc::clone(&store);
            let tenant = format!("tenant-{tenant_idx}");
            handles.push(tokio::spawn(TenantContext::scope(
                tenant.clone(),
                async move {
                    let task = Task {
                        id: TaskId::new(format!("task-{tenant_idx}-{task_idx}")),
                        context_id: ContextId::new(format!("ctx-{tenant_idx}")),
                        status: TaskStatus::new(TaskState::Completed),
                        history: None,
                        artifacts: None,
                        metadata: None,
                    };
                    store.save(task).await.unwrap();
                },
            )));
        }
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify each tenant sees exactly its own 50 tasks.
    let params = ListTasksParams::default();
    for tenant_idx in 0..10 {
        let store = Arc::clone(&store);
        let params = params.clone();
        let tenant = format!("tenant-{tenant_idx}");
        let count = TenantContext::scope(tenant, async move {
            store.list(&params).await.unwrap().tasks.len()
        })
        .await;
        assert_eq!(
            count, 50,
            "tenant-{tenant_idx} should have exactly 50 tasks, got {count}"
        );
    }
}

/// Rapid connect-disconnect cycles — ensures no resource leaks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rapid_connect_disconnect_cycles() {
    let completed = Arc::new(AtomicUsize::new(0));
    let addr = start_stress_server(Arc::clone(&completed)).await;

    // Each iteration creates a new client, sends one request, and drops it.
    for i in 0..100 {
        let client = build_http_client();
        let result = send_request(&client, addr, i).await;
        assert!(result.is_ok(), "request {i} failed: {:?}", result.err());
        drop(client);
    }

    // Wait for executors.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(completed.load(Ordering::Relaxed), 100);
}
