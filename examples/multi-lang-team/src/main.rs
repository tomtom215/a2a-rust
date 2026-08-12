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

mod surface;
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

/// Sends `user_text` to one worker agent and renders its reply (or the
/// failure) as a single line for the combined artifact.
async fn call_worker(worker: &Worker, user_text: &str) -> String {
    let client = match ClientBuilder::new(worker.url).build() {
        Ok(c) => c,
        Err(e) => return format!("[{}] Error connecting: {}", worker.language, e),
    };

    let params = MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text(user_text)],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };

    match tokio::time::timeout(Duration::from_secs(10), client.send_message(params)).await {
        Ok(Ok(response)) => match response {
            SendMessageResponse::Task(task) => task
                .artifacts
                .as_ref()
                .and_then(|arts| arts.first())
                .map(|a| extract_text(&a.parts))
                .unwrap_or_else(|| format!("[{}] (no artifact)", worker.language)),
            SendMessageResponse::Message(msg) => extract_text(&msg.parts),
            _ => format!("[{}] (unknown response type)", worker.language),
        },
        Ok(Err(e)) => format!("[{}] Error: {}", worker.language, e),
        Err(_) => format!("[{}] Timeout after 10s", worker.language),
    }
}

/// Message-text prefix that makes the coordinator pause mid-task.
///
/// `SubscribeToTask` is refused on a terminal task, correctly, so without a
/// slow turn its success path is unreachable.
const SLOW_PREFIX: &str = "slow:";

/// Coordinator executor that delegates to cross-language workers.
struct CoordinatorExecutor {
    /// Workers found reachable at startup.
    ///
    /// Probed once rather than discovered per request. With all four down, a
    /// per-request fan-out costs a full timeout window on *every* call, which
    /// makes the surface sweep take minutes and tells the reader nothing they
    /// were not told at startup. Empty means "delegate to nobody" — and the
    /// artifact says so, rather than presenting an empty result as a
    /// successful round-trip.
    reachable: Vec<&'static Worker>,
}

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

            // Fan out to all workers concurrently — a slow or down worker
            // costs one timeout window in total, not one per worker. Worker
            // failures are reported inline in the combined artifact
            // (partial results are this coordinator's contract); the task
            // itself only fails if the coordinator cannot make progress.
            if user_text.starts_with(SLOW_PREFIX) {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }

            let tasks: Vec<_> = self
                .reachable
                .iter()
                .map(|worker| {
                    let user_text = user_text.clone();
                    let worker: &'static Worker = worker;
                    tokio::spawn(async move { call_worker(worker, &user_text).await })
                })
                .collect();

            let mut responses = Vec::new();
            for (worker, task) in self.reachable.iter().zip(tasks) {
                responses.push(task.await.unwrap_or_else(|join_err| {
                    format!("[{}] Worker call panicked: {join_err}", worker.language)
                }));
            }
            if responses.is_empty() {
                responses.push(
                    "[no worker agents reachable — nothing was delegated] \
                     The coordinator answered on its own. This is not a \
                     cross-language round-trip; start the workers under \
                     itk/agents/ to make it one."
                        .to_owned(),
                );
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
            protocol_version: a2a_protocol_types::A2A_VERSION.into(),
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
            .with_push_notifications(true)
            .with_extended_agent_card(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// Serves the JSON-RPC dispatcher on an already-bound listener.
///
/// Binding before building the handler lets the agent card carry the real
/// address (instead of building a second, throwaway handler once the port
/// is known).
fn serve(listener: tokio::net::TcpListener, dispatcher: Arc<JsonRpcDispatcher>) {
    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => continue,
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

/// Probes each worker's agent-card endpoint.
///
/// Reported explicitly, never inferred. The coordinator answers with or
/// without workers, so "the demo ran" says nothing about whether any
/// cross-language delegation happened — which is the entire point of this
/// example.
async fn probe_workers() -> Vec<&'static Worker> {
    let mut up = Vec::new();
    for w in WORKERS {
        let url = format!("{}/.well-known/agent-card.json", w.url);
        let reachable = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            a2a_protocol_client::resolve_agent_card(w.url),
        )
        .await
        .is_ok_and(|r| r.is_ok());
        let _ = url;
        if reachable {
            up.push(w);
        }
    }
    up
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Multi-Language Agent Team Example");
    println!("=================================");
    println!();

    let reachable = probe_workers().await;
    println!("Worker agents:");
    for w in WORKERS {
        let up = reachable.iter().any(|r| r.language == w.language);
        println!(
            "  {:<11} {:<24} {}",
            w.language,
            w.url,
            if up { "REACHABLE" } else { "not reachable" }
        );
    }
    if reachable.is_empty() {
        println!();
        println!("No workers reachable — the coordinator will answer on its own and");
        println!("say so in its artifact. The A2A surface below is still fully");
        println!("measured; cross-language delegation is NOT. Start the workers:");
        println!("  Python: cd itk/agents/python    && python agent.py");
        println!("  JS:     cd itk/agents/js-agent  && node index.js");
        println!("  Go:     cd itk/agents/go-agent  && go run .");
        println!("  Java:   cd itk/agents/java-agent && mvn compile exec:java");
    }
    println!();

    // ── Server-only mode ─────────────────────────────────────────────────
    if let Ok(bind_addr) = std::env::var("A2A_BIND_ADDR") {
        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        let addr: SocketAddr = listener.local_addr()?;
        let handler = Arc::new(
            RequestHandlerBuilder::new(CoordinatorExecutor {
                reachable: reachable.clone(),
            })
            .with_agent_card(make_coordinator_card(&format!("http://{addr}")))
            .with_push_config_store(a2a_protocol_server::push::InMemoryPushConfigStore::new())
            .with_push_sender(a2a_protocol_server::push::HttpPushSender::new().allow_private_urls())
            .allow_unauthenticated_extended_card()
            .build()?,
        );
        serve(listener, Arc::new(JsonRpcDispatcher::new(handler)));
        println!("Coordinator listening on http://{addr}");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    // ── Surface run ──────────────────────────────────────────────────────
    let ep = surface::start(reachable.clone()).await?;
    let webhook = surface::start_webhook().await?;
    let restricted = surface::start_restricted().await?;

    println!("Coordinator:");
    println!("  JSON-RPC  {}", ep.jsonrpc);
    println!("  HTTP+JSON {}", ep.rest);
    println!("  gRPC      {}", ep.grpc);
    println!("  WebSocket {}", ep.websocket);
    println!();

    let mut clients = Vec::new();
    for binding in a2a_example_harness::Binding::ALL {
        match surface::client_for(*binding, &ep).await {
            Ok(c) => clients.push((*binding, c)),
            Err(e) => {
                eprintln!("could not build a {} client: {e}", binding.label());
                std::process::exit(1);
            }
        }
    }
    let main_client = ClientBuilder::new(&ep.jsonrpc).build()?;
    let restricted_client = ClientBuilder::new(&restricted).build()?;

    println!("=== Coverage: every A2A method over every binding ===\n");
    let outcome = a2a_example_harness::run_surface(a2a_example_harness::SurfaceRun {
        clients,
        main_client: &main_client,
        restricted_client: &restricted_client,
        webhook_url: webhook,
        slow_prefix: SLOW_PREFIX,
    })
    .await;

    println!();
    if reachable.is_empty() {
        println!("Reminder: cross-language delegation was NOT exercised — no worker");
        println!("agents were reachable. Only the coordinator's own A2A surface was.");
    } else {
        println!(
            "Cross-language delegation exercised against {} worker(s): {}.",
            reachable.len(),
            reachable
                .iter()
                .map(|w| w.language)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let code = outcome.exit_code();
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}
