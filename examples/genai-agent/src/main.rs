// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Example: Wrapping a `genai` LLM client behind the A2A protocol.
//!
//! This demonstrates how to integrate the Rust `genai` multi-provider LLM
//! client (<https://crates.io/crates/genai>) with the A2A protocol.
//!
//! `genai` supports OpenAI, Anthropic, Google Gemini, Ollama, Groq, and
//! Cohere out of the box. Any model name that doesn't match a hosted
//! provider's prefix routes to the Ollama adapter at
//! `http://localhost:11434/v1/`, which is also what llama.cpp's
//! `llama-server` speaks — so this example runs fully local with no API
//! key (see the README for a verified walkthrough).
//!
//! # Setup
//!
//! ```bash
//! # Hosted provider:
//! export OPENAI_API_KEY=sk-...
//! cargo run -p genai-a2a-agent
//!
//! # Fully local (Ollama or llama-server on :11434), no key needed:
//! GENAI_MODEL=qwen3-0.6b cargo run -p genai-a2a-agent
//! ```
//!
//! # How it works
//!
//! 1. The A2A server receives a `message/send` request.
//! 2. `GenaiAgentExecutor` extracts the user's text from the A2A message.
//!    A message with no text part fails the task with `InvalidParams`.
//! 3. The text is passed to `genai::Client` for LLM completion.
//! 4. On success the LLM response is returned as an A2A artifact and the
//!    task completes; on LLM failure the executor returns an error and the
//!    task transitions to `TASK_STATE_FAILED` — clients can rely on the
//!    task state instead of parsing artifact text for error strings.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
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

/// An A2A `AgentExecutor` that wraps a genai LLM client.
struct GenaiAgentExecutor {
    /// The genai client instance.
    client: genai::Client,
    /// The model to use (e.g., "gpt-4o-mini", "qwen3-0.6b").
    model: String,
}

impl GenaiAgentExecutor {
    fn new(model: impl Into<String>) -> Self {
        Self {
            client: genai::Client::default(),
            model: model.into(),
        }
    }
}

impl AgentExecutor for GenaiAgentExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // 1. Extract user text. A message without any text part is a
            //    client error — fail the task instead of prompting the LLM
            //    with an empty string.
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

            // 3. Call the LLM via genai. A provider failure propagates as an
            //    error so the server marks the task TASK_STATE_FAILED —
            //    never a "successful" task whose artifact hides an error.
            let chat_req = genai::chat::ChatRequest::new(vec![
                genai::chat::ChatMessage::system("You are a helpful A2A protocol agent."),
                genai::chat::ChatMessage::user(user_text),
            ]);

            let response = self
                .client
                .exec_chat(&self.model, chat_req, None)
                .await
                .map_err(|e| A2aError::internal(format!("LLM request failed: {e}")))?;
            let response_text = response
                .first_text()
                .ok_or_else(|| A2aError::internal("LLM returned no text content"))?;

            // 4. Package as A2A artifact
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("llm-response", vec![Part::text(response_text)]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // 5. Complete
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
        name: "Genai LLM Agent".into(),
        description: format!("A2A agent backed by the '{model}' model via genai"),
        version: env!("CARGO_PKG_VERSION").into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "chat".into(),
            name: "LLM Chat".into(),
            description: "Sends the message text to the configured LLM and returns the completion"
                .into(),
            tags: vec!["llm".into(), "chat".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = std::env::var("GENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let bind_addr = std::env::var("A2A_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:0".to_string());

    println!("Genai + A2A Agent Example");
    println!("=========================");
    println!();
    println!("Model: {model}");
    println!("Set GENAI_MODEL to change it. Unknown model names route to the");
    println!("Ollama adapter (http://localhost:11434/v1/) — run Ollama or");
    println!("llama-server there for a fully local, key-free agent.");
    println!();

    // Bind first so the agent card can advertise the real address.
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    let addr: SocketAddr = listener.local_addr()?;
    let url = format!("http://{addr}");

    let executor = GenaiAgentExecutor::new(&model);
    let handler = Arc::new(
        RequestHandlerBuilder::new(executor)
            .with_agent_card(make_agent_card(&url, &model))
            .with_push_config_store(InMemoryPushConfigStore::new())
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .build()?,
    );
    serve(listener, Arc::new(JsonRpcDispatcher::new(handler)));

    println!("Genai A2A agent listening on {url}");
    println!();
    println!("Test with:");
    println!("  cargo run -p a2a-tck -- --url {url} --binding jsonrpc");

    tokio::signal::ctrl_c().await?;
    println!("Shutting down.");

    Ok(())
}
