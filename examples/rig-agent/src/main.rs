// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Example: Wrapping a `rig` AI agent behind the A2A protocol.
//!
//! This demonstrates how to integrate the Rust `rig` AI framework
//! (<https://github.com/0xPlaygrounds/rig>) with the A2A protocol.
//!
//! # Architecture
//!
//! ```text
//! A2A Client ──→ A2A Server (a2a-protocol-server)
//!                     │
//!                     ▼
//!               RigAgentExecutor
//!                     │
//!                     ▼
//!               rig::Agent (LLM-powered)
//! ```
//!
//! # Setup
//!
//! Set the `OPENAI_API_KEY` environment variable for the rig OpenAI provider,
//! or modify this example to use a different rig provider.
//!
//! ```bash
//! export OPENAI_API_KEY=sk-...
//! cargo run -p rig-a2a-agent
//! ```
//!
//! # How it works
//!
//! 1. The A2A server receives a `message/send` request.
//! 2. `RigAgentExecutor` extracts the user's text from the A2A message.
//! 3. The text is passed to a rig `Agent` for LLM completion.
//! 4. The LLM response is packaged as an A2A artifact and returned.
//!
//! This pattern works with any `rig` agent type (completion, chat, RAG, etc.).

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

/// An A2A `AgentExecutor` that wraps a rig agent.
///
/// This is the key integration point: it bridges the A2A protocol's
/// event-based streaming model with rig's completion-based model.
///
/// ## Adapting for your use case
///
/// Replace `MockCompletion` with your actual rig model:
///
/// ```rust,ignore
/// use rig::providers::openai;
///
/// let client = openai::Client::from_env();
/// let model = client.agent("gpt-4o")
///     .preamble("You are a helpful assistant.")
///     .build();
/// ```
struct RigAgentExecutor {
    /// Description of the rig agent for status messages.
    agent_name: String,
}

impl RigAgentExecutor {
    fn new(name: impl Into<String>) -> Self {
        Self {
            agent_name: name.into(),
        }
    }

    /// Simulates a rig agent completion.
    ///
    /// In a real integration, this would call:
    /// ```rust,ignore
    /// let response = self.rig_agent.prompt(&user_text).await?;
    /// ```
    async fn run_rig_completion(&self, user_text: &str) -> Result<String, String> {
        // === REPLACE THIS WITH YOUR ACTUAL RIG AGENT ===
        //
        // Example with rig + OpenAI:
        //
        // ```rust,ignore
        // use rig::providers::openai;
        // use rig::completion::Prompt;
        //
        // let client = openai::Client::from_env();
        // let model = client.agent("gpt-4o")
        //     .preamble("You are a helpful assistant.")
        //     .build();
        //
        // let response = model.prompt(user_text).await
        //     .map_err(|e| format!("rig error: {e}"))?;
        // Ok(response)
        // ```
        //
        // Example with rig + Anthropic:
        //
        // ```rust,ignore
        // use rig::providers::anthropic;
        // use rig::completion::Prompt;
        //
        // let client = anthropic::Client::from_env();
        // let model = client.agent("claude-sonnet-4-20250514")
        //     .preamble("You are a helpful assistant.")
        //     .build();
        //
        // let response = model.prompt(user_text).await
        //     .map_err(|e| format!("rig error: {e}"))?;
        // Ok(response)
        // ```

        // Mock response for demonstration
        Ok(format!(
            "[{agent}] Processed: {text}",
            agent = self.agent_name,
            text = user_text
        ))
    }
}

impl AgentExecutor for RigAgentExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // 1. Extract user text from the A2A message
            let user_text = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text { text } => Some(text.as_str()),
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

            // 3. Run the rig agent
            let response = self
                .run_rig_completion(user_text)
                .await
                .unwrap_or_else(|e| format!("Error: {e}"));

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
    println!("Rig + A2A Agent Example");
    println!("=======================");
    println!();
    println!("This example shows how to wrap a rig AI agent behind the A2A protocol.");
    println!("Replace the mock completion in RigAgentExecutor::run_rig_completion()");
    println!("with your actual rig agent for real LLM integration.");
    println!();

    let executor = RigAgentExecutor::new("rig-demo-agent");

    // Build and start the server
    let handler = Arc::new(RequestHandlerBuilder::new(executor).build()?);
    let addr = start_server(handler).await;

    println!("Rig A2A agent listening on http://{addr}");
    println!();
    println!("Test with:");
    println!("  cargo run -p a2a-tck -- --url http://{addr}");
    println!();

    // Keep running until ctrl+c
    tokio::signal::ctrl_c().await?;
    println!("Shutting down.");

    Ok(())
}
