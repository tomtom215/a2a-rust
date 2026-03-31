// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Multi-language agent team example.
//!
//! Demonstrates a Rust coordinator agent that delegates work to worker agents
//! implemented in Python, JavaScript, Go, and Java — proving end-to-end
//! cross-language A2A interoperability.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐
//! │   Rust Coordinator   │  ← accepts user requests via A2A
//! │   (a2a-protocol-sdk) │
//! └──────┬──┬──┬──┬─────┘
//!        │  │  │  │
//!   ┌────┘  │  │  └────┐
//!   ▼       ▼  ▼       ▼
//! Python   JS  Go    Java    ← worker agents (each language)
//! :9100  :9101 :9102 :9103
//! ```
//!
//! # Running
//!
//! 1. Start the worker agents (from `itk/agents/`):
//!    ```bash
//!    cd itk/agents/python && python agent.py &
//!    cd itk/agents/js-agent && node index.js &
//!    cd itk/agents/go-agent && go run . &
//!    cd itk/agents/java-agent && mvn compile exec:java &
//!    ```
//!
//! 2. Run this coordinator:
//!    ```bash
//!    cargo run -p multi-lang-team
//!    ```

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

/// Worker agent configuration.
struct Worker {
    language: &'static str,
    url: &'static str,
}

const WORKERS: &[Worker] = &[
    Worker {
        language: "Python",
        url: "http://127.0.0.1:9100",
    },
    Worker {
        language: "JavaScript",
        url: "http://127.0.0.1:9101",
    },
    Worker {
        language: "Go",
        url: "http://127.0.0.1:9102",
    },
    Worker {
        language: "Java",
        url: "http://127.0.0.1:9103",
    },
];

fn extract_text(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|p| match &p.content {
            PartContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Coordinator executor that delegates to cross-language workers.
struct CoordinatorExecutor;

impl AgentExecutor for CoordinatorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let user_text = extract_text(&ctx.message.parts);

            // Set status to working
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // Fan out to all available workers
            let mut responses = Vec::new();
            for worker in WORKERS {
                let client = match ClientBuilder::new(worker.url).build() {
                    Ok(c) => c,
                    Err(e) => {
                        responses.push(format!("[{}] Error connecting: {}", worker.language, e));
                        continue;
                    }
                };

                let params = MessageSendParams {
                    tenant: None,
                    message: Message {
                        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
                        role: MessageRole::User,
                        parts: vec![Part::text(&user_text)],
                        task_id: None,
                        context_id: None,
                        reference_task_ids: None,
                        extensions: None,
                        metadata: None,
                    },
                    configuration: None,
                    metadata: None,
                };

                match tokio::time::timeout(Duration::from_secs(10), client.send_message(params))
                    .await
                {
                    Ok(Ok(response)) => {
                        let text = match response {
                            SendMessageResponse::Task(task) => task
                                .artifacts
                                .as_ref()
                                .and_then(|arts| arts.first())
                                .map(|a| extract_text(&a.parts))
                                .unwrap_or_else(|| format!("[{}] (no artifact)", worker.language)),
                            SendMessageResponse::Message(msg) => extract_text(&msg.parts),
                            _ => format!("[{}] (unknown response type)", worker.language),
                        };
                        responses.push(text);
                    }
                    Ok(Err(e)) => {
                        responses.push(format!("[{}] Error: {}", worker.language, e));
                    }
                    Err(_) => {
                        responses.push(format!("[{}] Timeout after 10s", worker.language));
                    }
                }
            }

            // Combine results into a single artifact
            let combined = responses.join("\n");
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("cross-lang-result", vec![Part::text(&combined)]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // Complete
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

fn make_coordinator_card(url: &str) -> AgentCard {
    AgentCard {
        url: None,
        name: "Multi-Language Coordinator".into(),
        description: "Coordinator that delegates to Python, JS, Go, and Java worker agents".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "coordinate".into(),
            name: "Cross-Language Coordination".into(),
            description: "Delegates work to agents in 4 languages".into(),
            tags: vec!["multi-lang".into(), "coordination".into()],
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

async fn start_server(handler: Arc<a2a_protocol_server::handler::RequestHandler>) -> SocketAddr {
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Multi-Language Agent Team Example");
    println!("=================================");
    println!();

    let handler = Arc::new(RequestHandlerBuilder::new(CoordinatorExecutor).build()?);

    let addr = start_server(Arc::clone(&handler)).await;
    println!("Coordinator listening on {addr}");

    // Update the handler's agent card with the real address
    let card = make_coordinator_card(&format!("http://{addr}"));
    let handler_with_card = Arc::new(
        RequestHandlerBuilder::new(CoordinatorExecutor)
            .with_agent_card(card)
            .build()?,
    );
    let addr = start_server(handler_with_card).await;
    println!("Coordinator (with card) listening on {addr}");

    // ── Demo: Send a request to the coordinator ────────────────────────────
    println!();
    println!("Sending request to coordinator...");

    let client = ClientBuilder::new(format!("http://{addr}")).build()?;
    let params = MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text("Hello from the multi-language team demo!")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };

    match client.send_message(params).await {
        Ok(response) => {
            println!();
            match response {
                SendMessageResponse::Task(task) => {
                    println!("Task ID: {}", task.id.0);
                    println!("State:   {}", task.status.state);
                    if let Some(artifacts) = &task.artifacts {
                        for artifact in artifacts {
                            println!();
                            println!(
                                "Artifact: {}",
                                artifact.name.as_deref().unwrap_or("(unnamed)")
                            );
                            println!("{}", extract_text(&artifact.parts));
                        }
                    }
                }
                SendMessageResponse::Message(msg) => {
                    println!("Direct message response:");
                    println!("  {}", extract_text(&msg.parts));
                }
                _ => println!("Unknown response type"),
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            eprintln!();
            eprintln!("Make sure the worker agents are running:");
            eprintln!("  Python: cd itk/agents/python && python agent.py");
            eprintln!("  JS:     cd itk/agents/js-agent && node index.js");
            eprintln!("  Go:     cd itk/agents/go-agent && go run .");
            eprintln!("  Java:   cd itk/agents/java-agent && mvn compile exec:java");
        }
    }

    Ok(())
}
