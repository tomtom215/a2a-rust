// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Example: Wrapping a `genai` LLM client behind the A2A protocol.
//!
//! This demonstrates how to integrate the Rust `genai` multi-provider LLM
//! client (<https://crates.io/crates/genai>) with the A2A protocol.
//!
//! `genai` supports OpenAI, Anthropic, Google Gemini, Ollama, Groq, and
//! Cohere out of the box.
//!
//! # Setup
//!
//! Set the appropriate API key for your provider:
//!
//! ```bash
//! export OPENAI_API_KEY=sk-...        # for OpenAI
//! export ANTHROPIC_API_KEY=sk-ant-... # for Anthropic
//! cargo run -p genai-a2a-agent
//! ```
//!
//! # How it works
//!
//! 1. The A2A server receives a `message/send` request.
//! 2. `GenaiAgentExecutor` extracts the user's text from the A2A message.
//! 3. The text is passed to `genai::Client` for LLM completion.
//! 4. The LLM response is packaged as an A2A artifact and returned.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

/// An A2A `AgentExecutor` that wraps a genai LLM client.
struct GenaiAgentExecutor {
    /// The genai client instance.
    client: genai::Client,
    /// The model to use (e.g., "gpt-4o", "claude-sonnet-4-20250514", "gemini-1.5-flash").
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
            // 1. Extract user text
            let user_text = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .unwrap_or("");

            // 2. Transition to Working
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // 3. Call the LLM via genai
            let chat_req = genai::chat::ChatRequest::new(vec![
                genai::chat::ChatMessage::system("You are a helpful A2A protocol agent."),
                genai::chat::ChatMessage::user(user_text),
            ]);

            let response_text = match self.client.exec_chat(&self.model, chat_req, None).await {
                Ok(response) => response
                    .content_text_as_str()
                    .unwrap_or("[no response text]")
                    .to_string(),
                Err(e) => format!("LLM error: {e}"),
            };

            // 4. Package as A2A artifact
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("llm-response", vec![Part::text(&response_text)]),
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
    let model = std::env::var("GENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

    println!("Genai + A2A Agent Example");
    println!("=========================");
    println!();
    println!("Model: {model}");
    println!(
        "Set GENAI_MODEL env var to change (e.g., claude-sonnet-4-20250514, gemini-1.5-flash)"
    );
    println!();

    let executor = GenaiAgentExecutor::new(&model);
    let handler = Arc::new(RequestHandlerBuilder::new(executor).build()?);
    let addr = start_server(handler).await;

    println!("Genai A2A agent listening on http://{addr}");
    println!();
    println!("Test with:");
    println!("  cargo run -p a2a-tck -- --url http://{addr}");

    tokio::signal::ctrl_c().await?;
    println!("Shutting down.");

    Ok(())
}
