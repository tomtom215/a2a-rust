// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! End-to-end A2A echo agent example.
//!
//! This example demonstrates the full A2A protocol stack:
//!
//! 1. **Agent executor** — Implements [`AgentExecutor`] to echo incoming
//!    messages back as artifacts with status updates.
//! 2. **Server** — Starts a hyper HTTP server with both JSON-RPC and REST
//!    dispatchers on separate ports.
//! 3. **Client** — Connects to the server and exercises synchronous send,
//!    streaming send, task retrieval, and agent card discovery.
//!
//! Run with: `cargo run -p echo-agent`

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use a2a_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_types::artifact::Artifact;
use a2a_types::error::A2aResult;
use a2a_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_types::message::{Message, MessageId, MessageRole, Part};
use a2a_types::params::MessageSendParams;
use a2a_types::responses::SendMessageResponse;
use a2a_types::task::{ContextId, TaskState, TaskStatus};

use a2a_client::ClientBuilder;
use a2a_server::builder::RequestHandlerBuilder;
use a2a_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_server::executor::AgentExecutor;
use a2a_server::request_context::RequestContext;
use a2a_server::streaming::EventQueueWriter;

// ── Echo executor ────────────────────────────────────────────────────────────

/// A simple agent that echoes the first text part of the incoming message
/// back as an artifact, going through Working → Completed status transitions.
struct EchoExecutor;

impl AgentExecutor for EchoExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Transition to Working.
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // Extract text from the incoming message.
            let input_text = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    a2a_types::message::PartContent::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .unwrap_or("<no text>");

            // Echo back as an artifact.
            let echo_text = format!("Echo: {input_text}");
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("echo-artifact", vec![Part::text(&echo_text)]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // Transition to Completed.
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

// ── Agent card ───────────────────────────────────────────────────────────────

fn make_agent_card(jsonrpc_url: &str, rest_url: &str) -> AgentCard {
    AgentCard {
        name: "Echo Agent".into(),
        description: "A simple echo agent that mirrors your input".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![
            AgentInterface {
                url: jsonrpc_url.into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            },
            AgentInterface {
                url: rest_url.into(),
                protocol_binding: "REST".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            },
        ],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "echo".into(),
            name: "Echo".into(),
            description: "Echoes your message back as an artifact".into(),
            tags: vec!["echo".into(), "demo".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

// ── Server startup ───────────────────────────────────────────────────────────

async fn start_jsonrpc_server(handler: Arc<a2a_server::handler::RequestHandler>) -> SocketAddr {
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind JSON-RPC listener");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
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

    addr
}

async fn start_rest_server(handler: Arc<a2a_server::handler::RequestHandler>) -> SocketAddr {
    let dispatcher = Arc::new(RestDispatcher::new(handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind REST listener");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
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

    addr
}

// ── Client helpers ───────────────────────────────────────────────────────────

fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
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

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // When the `tracing` feature is enabled, initialize a subscriber that
    // prints structured logs to stderr. Set RUST_LOG=debug for verbose output.
    #[cfg(feature = "tracing")]
    {
        use tracing_subscriber::EnvFilter;
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .init();
    }

    println!("=== A2A Echo Agent Example ===\n");

    // Build the shared request handler.
    let handler = Arc::new(
        RequestHandlerBuilder::new(EchoExecutor)
            .with_agent_card(make_agent_card(
                "http://placeholder", // Updated after binding
                "http://placeholder",
            ))
            .build()
            .expect("build handler"),
    );

    // Start servers on random ports.
    let jsonrpc_addr = start_jsonrpc_server(Arc::clone(&handler)).await;
    let rest_addr = start_rest_server(Arc::clone(&handler)).await;

    println!("JSON-RPC server listening on http://{jsonrpc_addr}");
    println!("REST server listening on http://{rest_addr}\n");

    // ── Demo 1: Synchronous SendMessage via JSON-RPC ─────────────────────

    println!("--- Demo 1: Synchronous SendMessage (JSON-RPC) ---");
    let client = ClientBuilder::new(format!("http://{jsonrpc_addr}"))
        .build()
        .expect("build client");

    let response = client
        .send_message(make_send_params("Hello from JSON-RPC client!"))
        .await
        .expect("send_message");

    match &response {
        SendMessageResponse::Task(task) => {
            println!("  Task ID:    {}", task.id);
            println!("  Status:     {:?}", task.status.state);
            if let Some(artifacts) = &task.artifacts {
                for art in artifacts {
                    println!("  Artifact:   {}", art.id);
                    for part in &art.parts {
                        if let a2a_types::message::PartContent::Text { text } = &part.content {
                            println!("  Content:    {text}");
                        }
                    }
                }
            }
        }
        SendMessageResponse::Message(msg) => {
            println!("  Got immediate message: {msg:?}");
        }
        _ => {}
    }
    println!();

    // ── Demo 2: Streaming SendMessage via JSON-RPC ───────────────────────

    println!("--- Demo 2: Streaming SendMessage (JSON-RPC) ---");
    let mut stream = client
        .stream_message(make_send_params("Hello from streaming client!"))
        .await
        .expect("stream_message");

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamResponse::StatusUpdate(ev)) => {
                println!("  Status update: {:?}", ev.status.state);
            }
            Ok(StreamResponse::ArtifactUpdate(ev)) => {
                println!("  Artifact update: {}", ev.artifact.id);
                for part in &ev.artifact.parts {
                    if let a2a_types::message::PartContent::Text { text } = &part.content {
                        println!("  Content:    {text}");
                    }
                }
            }
            Ok(StreamResponse::Task(task)) => {
                println!("  Task snapshot: {} ({:?})", task.id, task.status.state);
            }
            Ok(StreamResponse::Message(msg)) => {
                println!("  Message: {msg:?}");
            }
            Ok(_) => {
                // Future stream response variants — ignore gracefully.
            }
            Err(e) => {
                println!("  Stream error: {e}");
                break;
            }
        }
    }
    println!();

    // ── Demo 3: Synchronous SendMessage via REST ─────────────────────────

    println!("--- Demo 3: Synchronous SendMessage (REST) ---");
    let rest_client = ClientBuilder::new(format!("http://{rest_addr}"))
        .with_protocol_binding("REST")
        .build()
        .expect("build REST client");

    let response = rest_client
        .send_message(make_send_params("Hello from REST client!"))
        .await
        .expect("send_message REST");

    match &response {
        SendMessageResponse::Task(task) => {
            println!("  Task ID:    {}", task.id);
            println!("  Status:     {:?}", task.status.state);
            if let Some(artifacts) = &task.artifacts {
                for art in artifacts {
                    for part in &art.parts {
                        if let a2a_types::message::PartContent::Text { text } = &part.content {
                            println!("  Content:    {text}");
                        }
                    }
                }
            }
        }
        SendMessageResponse::Message(msg) => {
            println!("  Got immediate message: {msg:?}");
        }
        _ => {}
    }
    println!();

    // ── Demo 4: Streaming via REST ───────────────────────────────────────

    println!("--- Demo 4: Streaming SendMessage (REST) ---");
    let mut stream = rest_client
        .stream_message(make_send_params("Hello from REST streaming!"))
        .await
        .expect("stream_message REST");

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamResponse::StatusUpdate(ev)) => {
                println!("  Status update: {:?}", ev.status.state);
            }
            Ok(StreamResponse::ArtifactUpdate(ev)) => {
                for part in &ev.artifact.parts {
                    if let a2a_types::message::PartContent::Text { text } = &part.content {
                        println!("  Content:    {text}");
                    }
                }
            }
            Ok(_) => {}
            Err(e) => {
                println!("  Stream error: {e}");
                break;
            }
        }
    }
    println!();

    // ── Demo 5: GetTask ──────────────────────────────────────────────────

    println!("--- Demo 5: GetTask ---");
    // Use the task ID from Demo 1.
    if let SendMessageResponse::Task(task) = &response {
        let fetched = client
            .get_task(a2a_types::params::TaskQueryParams {
                tenant: None,
                id: task.id.to_string(),
                history_length: None,
            })
            .await
            .expect("get_task");
        println!(
            "  Fetched task: {} ({:?})",
            fetched.id, fetched.status.state
        );
    }
    println!();

    println!("=== All demos completed successfully! ===");
}
