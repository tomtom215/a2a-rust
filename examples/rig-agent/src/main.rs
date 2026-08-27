// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Example: wrapping a [`rig`](https://github.com/0xPlaygrounds/rig) agent
//! behind the A2A protocol.
//!
//! A real `rig-core` agent (OpenAI-compatible provider) serves A2A traffic:
//! incoming `SendMessage` text is passed to [`rig_core::completion::Prompt`],
//! and the completion comes back as an A2A artifact. The executor is
//! generic over [`rig_core::completion::CompletionModel`], so the same bridge
//! works with any rig provider (Anthropic, Gemini, Ollama, …) — swap the
//! client construction in `main` and nothing else changes.
//!
//! # Architecture
//!
//! ```text
//! A2A Client ──→ A2A Server (a2a-protocol-server)
//!                     │
//!                     ▼
//!               RigAgentExecutor<M>
//!                     │
//!                     ▼
//!               rig_core::agent::Agent<M> ──→ LLM provider
//! ```
//!
//! # Setup
//!
//! ```bash
//! # Hosted OpenAI:
//! export OPENAI_API_KEY=sk-...
//! cargo run -p rig-a2a-agent
//!
//! # Fully local (any OpenAI-compatible server, e.g. llama.cpp's
//! # llama-server or Ollama), no real key needed:
//! export OPENAI_API_KEY=local
//! export OPENAI_BASE_URL=http://127.0.0.1:11434/v1
//! RIG_MODEL=qwen3.5:0.8b cargo run -p rig-a2a-agent
//! ```
//!
//! # Failure semantics
//!
//! A message without a text part fails the task with `InvalidParams`; a
//! provider error fails it with an internal error. Clients observe
//! `TASK_STATE_FAILED` — errors are never disguised as successful tasks.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;

mod surface;

#[cfg(test)]
mod tests;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::push::{HttpPushSender, InMemoryPushConfigStore};
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use rig_core::client::{CompletionClient, ProviderClient};
use rig_core::completion::Prompt;
use rig_core::providers::openai;

/// An A2A `AgentExecutor` that delegates to a rig [`Agent`].
///
/// Generic over the rig completion model, so any provider rig supports can
/// sit behind the same A2A bridge.
///
/// [`Agent`]: rig_core::agent::Agent
/// Message-text prefix that makes the executor pause mid-task.
///
/// `SubscribeToTask` is refused on a terminal task, correctly, so without a
/// slow turn its success path is unreachable and only the refusal is observed.
const SLOW_PREFIX: &str = "slow:";

struct RigAgentExecutor<M: rig_core::completion::CompletionModel> {
    agent: rig_core::agent::Agent<M>,
    /// When `true`, a provider error produces a labelled mechanical reply
    /// instead of failing the task. Set only for the surface run, so the A2A
    /// protocol can be measured with no model reachable. Server mode leaves it
    /// `false`, because failing the task is correct for a real agent.
    fallback_on_error: bool,
}

impl<M> AgentExecutor for RigAgentExecutor<M>
where
    M: rig_core::completion::CompletionModel + 'static,
{
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // 1. Extract user text from the A2A message. No text part is a
            //    client error — fail the task rather than prompting with "".
            let user_text = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .ok_or_else(|| A2aError::invalid_params("message contains no text part"))?;

            // 2. Transition to Working
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // 3. Run the rig agent. Provider errors propagate so the server
            //    marks the task TASK_STATE_FAILED.
            if user_text.starts_with(SLOW_PREFIX) {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }

            let response = match self.agent.prompt(user_text).await {
                Ok(r) => r,
                // No provider reachable. Degrade to a *labelled* mechanical
                // reply so the protocol mechanics stay demonstrable with no
                // model at all. The label is load-bearing: without it, "the
                // agent answered" and "the model answered" become
                // indistinguishable.
                Err(e) if self.fallback_on_error => format!(
                    "[no model reachable — mechanical fallback, not an LLM answer] \
                     echo of your input: {user_text}\n(underlying error: {e})"
                ),
                Err(e) => {
                    return Err(A2aError::internal(format!("rig agent error: {e}")));
                }
            };

            // 4. Package the response as an A2A artifact
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("rig-response", vec![Part::text(&response)]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // 5. Transition to Completed
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

/// Builds the agent card advertised at `/.well-known/agent-card.json`.
fn make_agent_card(url: &str, model: &str) -> AgentCard {
    AgentCard {
        url: Some(url.into()),
        name: "Rig LLM Agent".into(),
        description: format!("A2A agent backed by the '{model}' model via rig"),
        version: env!("CARGO_PKG_VERSION").into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: a2a_protocol_types::A2A_VERSION.into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "chat".into(),
            name: "LLM Chat".into(),
            description: "Sends the message text to the rig agent and returns the completion"
                .into(),
            tags: vec!["llm".into(), "rig".into(), "chat".into()],
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
/// address.
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = std::env::var("RIG_MODEL").unwrap_or_else(|_| "qwen3.5:0.8b".to_string());

    println!("Rig + A2A Agent Example");
    println!("=======================");
    println!();
    println!("Model: {model} (set RIG_MODEL to change)");
    println!("Provider: OPENAI_API_KEY / OPENAI_BASE_URL — point the base URL at");
    println!("llama.cpp's llama-server, vLLM or Ollama for a local agent.");
    println!();

    // rig's OpenAI client refuses to build without OPENAI_API_KEY. Local
    // servers ignore the value, so default it rather than making the surface
    // run impossible to start — and say so, instead of pretending a key was
    // configured.
    if std::env::var("OPENAI_API_KEY").is_err() {
        println!("OPENAI_API_KEY unset — defaulting to a placeholder, which local");
        println!("OpenAI-compatible servers ignore. Hosted providers will reject it.");
        std::env::set_var("OPENAI_API_KEY", "local");
        println!();
    }

    let build_agent = |model: &str| -> Result<_, String> {
        let client = openai::CompletionsClient::from_env()
            .map_err(|e| format!("failed to build the rig OpenAI client: {e}"))?;
        Ok(client
            .agent(model)
            .preamble("You are a helpful A2A protocol agent.")
            .build())
    };

    // ── Server-only mode ─────────────────────────────────────────────────
    // The real deployment shape: one binding, and a provider error FAILS the
    // task rather than answering mechanically.
    if let Ok(bind_addr) = std::env::var("A2A_BIND_ADDR") {
        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        let addr: SocketAddr = listener.local_addr()?;
        let url = format!("http://{addr}");
        let handler = Arc::new(
            RequestHandlerBuilder::new(RigAgentExecutor {
                agent: build_agent(&model)?,
                fallback_on_error: false,
            })
            .with_agent_card(make_agent_card(&url, &model))
            .with_push_config_store(InMemoryPushConfigStore::new())
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .allow_unauthenticated_extended_card()
            .build()?,
        );
        serve(listener, Arc::new(JsonRpcDispatcher::new(handler)));
        println!("Rig A2A agent listening on {url}");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    // ── Surface run ──────────────────────────────────────────────────────
    let ep = surface::start(&model, || build_agent(&model)).await?;
    let webhook = surface::start_webhook().await?;
    let restricted = surface::start_restricted(&model, || build_agent(&model)).await?;

    println!("JSON-RPC  {}", ep.jsonrpc);
    println!("HTTP+JSON {}", ep.rest);
    println!("gRPC      {}", ep.grpc);
    println!("WebSocket {}", ep.websocket);
    println!();

    // Asked once, up front, and reported either way. The fallback keeps the
    // protocol working with no provider, so a green sweep says nothing about
    // the model — inferring one from the other is the substitution this
    // repository keeps removing.
    let llm_reachable = surface::probe_model(&ep.jsonrpc).await;
    if llm_reachable {
        println!("LLM leg: EXERCISED — '{model}' answered a real request.");
    } else {
        println!("LLM leg: NOT EXERCISED — no provider reachable, so every answer");
        println!("  below is the labelled mechanical fallback. The A2A surface is");
        println!("  still fully measured; the rig integration is not.");
    }
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
    let main_client = a2a_protocol_client::ClientBuilder::new(&ep.jsonrpc).build()?;
    let restricted_client = a2a_protocol_client::ClientBuilder::new(&restricted).build()?;

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
    if llm_reachable {
        println!("LLM leg exercised against '{model}'.");
    } else {
        println!("Reminder: the LLM leg was NOT exercised in this run.");
    }

    let code = outcome.exit_code();
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}
