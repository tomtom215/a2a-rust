// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! End-to-end A2A echo agent example.
//!
//! This example demonstrates the full A2A protocol stack:
//!
//! 1. **Agent executor** — Implements [`AgentExecutor`] to echo incoming
//!    messages back as artifacts with status updates.
//! 2. **Server** — Starts a hyper HTTP server with both JSON-RPC and REST
//!    dispatchers on separate ports (with correct URLs via pre-binding).
//! 3. **Client** — Connects to the server and exercises synchronous send,
//!    streaming send, agent card discovery, and task retrieval.
//!
//! Run with: `cargo run -p echo-agent`

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_client::{resolve_agent_card, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

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
                    a2a_protocol_types::message::PartContent::Text(text) => Some(text.as_str()),
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
        url: None,
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

/// Pre-binds a TCP listener to an ephemeral port. Returns the listener and
/// address so agent cards can be built with the correct URL before the handler
/// is constructed.
async fn bind_listener() -> (tokio::net::TcpListener, SocketAddr) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    (listener, addr)
}

fn serve_jsonrpc(
    listener: tokio::net::TcpListener,
    handler: Arc<a2a_protocol_server::handler::RequestHandler>,
) {
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => continue, // Transient errors should not kill the server
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
}

fn serve_rest(
    listener: tokio::net::TcpListener,
    handler: Arc<a2a_protocol_server::handler::RequestHandler>,
) {
    let dispatcher = Arc::new(RestDispatcher::new(handler));

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => continue, // Transient errors should not kill the server
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
}

/// Start a combined server serving both JSON-RPC and REST on a single address.
async fn start_combined_server(
    handler: Arc<a2a_protocol_server::handler::RequestHandler>,
    bind_addr: &str,
) -> SocketAddr {
    let jsonrpc = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));
    let rest = Arc::new(RestDispatcher::new(handler));

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .unwrap_or_else(|e| panic!("bind {bind_addr}: {e}"));
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => continue, // Transient errors should not kill the server
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let jsonrpc = Arc::clone(&jsonrpc);
            let rest = Arc::clone(&rest);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                    let jsonrpc = Arc::clone(&jsonrpc);
                    let rest = Arc::clone(&rest);
                    async move {
                        // Route: POST with JSON to root → JSON-RPC; everything else → REST.
                        let is_jsonrpc = req.method() == hyper::Method::POST
                            && (req.uri().path() == "/" || req.uri().path().is_empty());
                        if is_jsonrpc {
                            Ok::<_, std::convert::Infallible>(jsonrpc.dispatch(req).await)
                        } else {
                            Ok::<_, std::convert::Infallible>(rest.dispatch(req).await)
                        }
                    }
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

    // ── Server-only mode (for TCK / CI) ────────────────────────────────────
    // When A2A_BIND_ADDR is set, start a combined JSON-RPC + REST server on
    // that address and block forever.  No client demos are run.
    if let Ok(bind_addr) = std::env::var("A2A_BIND_ADDR") {
        let url = format!("http://{bind_addr}");
        let mut card = make_agent_card(&url, &url);
        card.url = Some(url);
        card.capabilities = AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true);
        let handler = Arc::new(
            RequestHandlerBuilder::new(EchoExecutor)
                .with_agent_card(card)
                .with_push_config_store(a2a_protocol_server::push::InMemoryPushConfigStore::new())
                .with_push_sender(
                    a2a_protocol_server::push::HttpPushSender::new().allow_private_urls(),
                )
                .build()
                .expect("build handler"),
        );

        let addr = start_combined_server(handler, &bind_addr).await;
        println!("Echo agent listening on http://{addr} (combined JSON-RPC + REST)");

        // Block forever — the server runs in background tasks.
        std::future::pending::<()>().await;
    }

    println!("=== A2A Echo Agent Example ===\n");

    // ── Pre-bind listeners to get addresses for agent cards ───────────────
    // This ensures the agent card contains the correct URL *before* the
    // handler is constructed (avoids placeholder URL bugs).
    let (jsonrpc_listener, jsonrpc_addr) = bind_listener().await;
    let (rest_listener, rest_addr) = bind_listener().await;

    let jsonrpc_url = format!("http://{jsonrpc_addr}");
    let rest_url = format!("http://{rest_addr}");

    // Build the shared request handler with correct URLs.
    let handler = Arc::new(
        RequestHandlerBuilder::new(EchoExecutor)
            .with_agent_card(make_agent_card(&jsonrpc_url, &rest_url))
            .build()
            .expect("build handler"),
    );

    // Start servers on pre-bound listeners.
    serve_jsonrpc(jsonrpc_listener, Arc::clone(&handler));
    serve_rest(rest_listener, Arc::clone(&handler));

    println!("JSON-RPC server listening on {jsonrpc_url}");
    println!("REST server listening on {rest_url}\n");

    // ── Demo 1: Synchronous SendMessage via JSON-RPC ─────────────────────

    println!("--- Demo 1: Synchronous SendMessage (JSON-RPC) ---");
    let client = ClientBuilder::new(&jsonrpc_url)
        .build()
        .expect("build client");

    let jsonrpc_response = client
        .send_message(make_send_params("Hello from JSON-RPC client!"))
        .await
        .expect("send_message");

    match &jsonrpc_response {
        SendMessageResponse::Task(task) => {
            println!("  Task ID:    {}", task.id);
            println!("  Status:     {:?}", task.status.state);
            if let Some(artifacts) = &task.artifacts {
                for art in artifacts {
                    println!("  Artifact:   {}", art.id);
                    for part in &art.parts {
                        if let a2a_protocol_types::message::PartContent::Text(text) = &part.content
                        {
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
                    if let a2a_protocol_types::message::PartContent::Text(text) = &part.content {
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
    let rest_client = ClientBuilder::new(&rest_url)
        .with_protocol_binding("REST")
        .build()
        .expect("build REST client");

    let rest_response = rest_client
        .send_message(make_send_params("Hello from REST client!"))
        .await
        .expect("send_message REST");

    match &rest_response {
        SendMessageResponse::Task(task) => {
            println!("  Task ID:    {}", task.id);
            println!("  Status:     {:?}", task.status.state);
            if let Some(artifacts) = &task.artifacts {
                for art in artifacts {
                    for part in &art.parts {
                        if let a2a_protocol_types::message::PartContent::Text(text) = &part.content
                        {
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
                    if let a2a_protocol_types::message::PartContent::Text(text) = &part.content {
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

    // ── Demo 5: Agent Card Discovery ─────────────────────────────────────
    // The server exposes /.well-known/agent-card.json — the client can discover
    // the agent's capabilities without any prior configuration.

    println!("--- Demo 5: Agent Card Discovery ---");
    match resolve_agent_card(&jsonrpc_url).await {
        Ok(card) => {
            println!("  Agent:      {}", card.name);
            println!("  Version:    {}", card.version);
            println!(
                "  Skills:     {:?}",
                card.skills.iter().map(|s| &s.name).collect::<Vec<_>>()
            );
            println!(
                "  Streaming:  {}",
                card.capabilities.streaming.unwrap_or(false)
            );
            println!("  Interfaces: {}", card.supported_interfaces.len());
        }
        Err(e) => {
            println!("  Discovery failed: {e}");
        }
    }
    println!();

    // ── Demo 6: GetTask ──────────────────────────────────────────────────
    // Retrieve the task created in Demo 1 by its ID.

    println!("--- Demo 6: GetTask ---");
    if let SendMessageResponse::Task(task) = &jsonrpc_response {
        let fetched = client
            .get_task(a2a_protocol_types::params::TaskQueryParams {
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
