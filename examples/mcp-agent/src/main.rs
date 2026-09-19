// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Example: an A2A agent whose tools come from an MCP server.
//!
//! Two protocols, one agent, and neither knows about the other:
//!
//! ```text
//!   A2A client ──JSON-RPC──→ this agent ──MCP over stdio──→ mcp-tool-server
//!              ←──task──────            ←──tool result──────  (child process)
//!                                  │
//!                                  └──HTTP──→ model provider
//! ```
//!
//! **A2A is how agents talk to each other. MCP is how one agent reaches its
//! tools.** They meet only here, in one process, and the A2A protocol never
//! carries a tool call while the MCP session never carries a task. That split
//! is the A2A project's own recommendation — *"A2A handles inter-agent
//! collaboration and MCP handles tool integration"* — and this example is what
//! it looks like in Rust.
//!
//! # What makes this different from `examples/rig-agent`
//!
//! `rig-agent` compiles its two tools in. This one has **none**: it spawns
//! `mcp-tool-server`, asks what it offers, and builds its catalogue and its
//! agent card from the answer. Point `MCP_SERVER_BIN` at a different MCP
//! server — someone else's, in any language — and the agent works with that
//! server's tools instead, unchanged.
//!
//! The tool loop in [`agent`] is otherwise identical to `rig-agent`'s, which
//! is the point: diff the two files and the only difference is where a tool
//! call goes.
//!
//! # Setup
//!
//! ```bash
//! # Fully local. --jinja is required or no model emits a tool call.
//! llama-server -m Qwen3-1.7B-Q4_K_M.gguf --port 11434 --alias qwen3:1.7b --jinja &
//!
//! export OPENAI_API_KEY=local
//! export OPENAI_BASE_URL=http://127.0.0.1:11434/v1
//! cargo run -p mcp-a2a-agent                      # self-driving demo
//! A2A_BIND_ADDR=127.0.0.1:8080 cargo run -p mcp-a2a-agent   # serve
//! ```

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};
use rig_core::client::CompletionClient;
use rig_core::completion::CompletionModel;
use rig_core::providers::openai;

mod agent;
mod mcp;

#[cfg(test)]
mod tests;

use agent::McpAgent;

/// An A2A `AgentExecutor` that delegates to an [`McpAgent`].
struct McpAgentExecutor<M: CompletionModel> {
    agent: McpAgent<M>,
    /// When `true`, a *model* error produces a labelled mechanical reply
    /// instead of failing the task, so the demo runs with no provider.
    ///
    /// Deliberately never applied to an MCP transport failure. A missing
    /// model is a degraded answer the label can warn about; a missing tool
    /// server means the answer would be invented, and there is no label that
    /// makes that acceptable.
    fallback_on_model_error: bool,
}

impl<M> AgentExecutor for McpAgentExecutor<M>
where
    M: CompletionModel + Clone + 'static,
{
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let user_text = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .ok_or_else(|| A2aError::invalid_params("message contains no text part"))?;

            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            let answer = match self.agent.prompt(user_text).await {
                Ok(answer) => answer,
                Err(agent::AgentError::Completion(e)) if self.fallback_on_model_error => {
                    agent::Answer {
                        text: format!(
                            "[no model reachable — mechanical fallback, not an LLM answer] \
                             echo of your input: {user_text}\n(underlying error: {e})"
                        ),
                        trace: Vec::new(),
                    }
                }
                Err(e) => return Err(A2aError::internal(format!("mcp agent error: {e}"))),
            };

            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("mcp-response", vec![Part::text(&answer.text)]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // The trace names the MCP calls that produced the answer. Same
            // reasoning as `examples/rig-agent`: without it a caller cannot
            // tell a researched answer from a guessed one. It matters more
            // here, because the tools are another process's and the caller
            // has no other way to see that it was reached at all.
            if !answer.trace.is_empty() {
                queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact: Artifact::new(
                            "tool-trace",
                            vec![Part::text(answer.trace.join("\n"))],
                        ),
                        append: None,
                        last_chunk: Some(true),
                        metadata: None,
                    }))
                    .await?;
            }

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

/// Where the bundled MCP server binary is.
///
/// Beside this one, because cargo puts every binary of a package in the same
/// directory — so the lookup holds for `cargo run`, for `target/release`, and
/// for a copied-out pair, without a build-script-baked path that would break
/// the moment either moved. `MCP_SERVER_BIN` overrides it, which is also how
/// you point this agent at somebody else's MCP server.
fn tool_server_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Ok(configured) = std::env::var("MCP_SERVER_BIN") {
        return Ok(PathBuf::from(configured));
    }
    let mut path = std::env::current_exe()?
        .parent()
        .ok_or("the running binary has no parent directory")?
        .join("mcp-tool-server");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    Ok(path)
}

/// Builds the agent card from what the MCP server actually offered.
///
/// The skills list is derived, not written: a card claiming a skill the
/// server does not serve is a lie told to every client that reads it, and
/// deriving it makes that impossible rather than merely discouraged.
fn make_agent_card(url: &str, model: &str, tools: &[&str]) -> AgentCard {
    AgentCard::new(
        "MCP-backed A2A Agent",
        env!("CARGO_PKG_VERSION"),
        AgentInterface::jsonrpc(url),
    )
    .with_description(format!(
        "A2A agent backed by the '{model}' model, with tools from an MCP server"
    ))
    .with_input_modes(["text/plain"])
    .with_output_modes(["text/plain"])
    .with_skill(
        AgentSkill::new(
            "mcp-tools",
            "MCP-backed question answering",
            format!(
                "Answers questions using the MCP server's tools: {}",
                tools.join(", ")
            ),
        )
        .with_tags(["llm", "mcp", "tool-calling"]),
    )
    .with_capabilities(AgentCapabilities::none().with_streaming(true))
}

/// Serves the JSON-RPC dispatcher on an already-bound listener.
fn serve(listener: tokio::net::TcpListener, dispatcher: Arc<JsonRpcDispatcher>) {
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
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
    let model_name = std::env::var("RIG_MODEL").unwrap_or_else(|_| "qwen3:1.7b".to_string());
    let server_bin = tool_server_path()?;

    println!("MCP + A2A Agent Example");
    println!("=======================");
    println!();
    println!("Model:      {model_name} (set RIG_MODEL to change)");
    println!(
        "MCP server: {} (set MCP_SERVER_BIN to change)",
        server_bin.display()
    );
    println!();

    // 1. Spawn the MCP server and discover what it can do. Before the model,
    //    before the listener: an agent that cannot reach its tools should
    //    fail to start rather than serve a card it cannot honour.
    // No `kill_on_drop` here: rmcp's own `ChildWithCleanup::drop` kills the
    // child rather than leaving a zombie, and stacking a second mechanism on
    // top would only obscure which one is responsible.
    let command = tokio::process::Command::new(&server_bin);
    let transport = rmcp::transport::TokioChildProcess::new(command)
        .map_err(|e| format!("could not spawn {}: {e}", server_bin.display()))?;
    let tools = mcp::McpTools::connect(transport).await?;
    println!("MCP tools discovered: {}", tools.names().join(", "));
    println!();

    // 2. The model. rig's OpenAI client refuses to build without a key;
    //    local servers ignore the value, so default it and say so rather than
    //    pretending one was configured.
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| {
        println!("OPENAI_API_KEY unset — defaulting to a placeholder, which local");
        println!("OpenAI-compatible servers ignore. Hosted providers will reject it.");
        println!();
        "local".to_owned()
    });
    let mut builder = openai::CompletionsClient::builder().api_key(&api_key);
    if let Ok(base_url) = std::env::var("OPENAI_BASE_URL") {
        builder = builder.base_url(&base_url);
    }
    let client = builder
        .build()
        .map_err(|e| format!("failed to build the rig OpenAI client: {e}"))?;

    let tool_names: Vec<String> = tools.names().into_iter().map(str::to_owned).collect();
    let agent = McpAgent::new(
        client.completion_model(&model_name),
        "You are a helpful A2A agent. Use the tools available to you to answer \
         questions about services; do not guess a service's state.",
        tools,
    );

    let bind_addr = std::env::var("A2A_BIND_ADDR");
    let listener =
        tokio::net::TcpListener::bind(bind_addr.as_deref().unwrap_or("127.0.0.1:0")).await?;
    let addr: SocketAddr = listener.local_addr()?;
    let url = format!("http://{addr}");
    let card_tools: Vec<&str> = tool_names.iter().map(String::as_str).collect();
    let handler = Arc::new(
        RequestHandlerBuilder::new(McpAgentExecutor {
            agent,
            fallback_on_model_error: bind_addr.is_err(),
        })
        .with_agent_card(make_agent_card(&url, &model_name, &card_tools))
        .build()?,
    );
    serve(listener, Arc::new(JsonRpcDispatcher::new(handler)));

    if bind_addr.is_ok() {
        println!("MCP A2A agent listening on {url}");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    // 3. No bind address: drive ourselves over a real A2A client, so the
    //    example demonstrates the whole path rather than describing it.
    println!("Demo — driving the agent over A2A at {url}\n");
    let a2a = a2a_protocol_client::ClientBuilder::new(&url).build()?;
    let mut failures = 0_u8;
    let mut used_tools = 0_u8;
    for question in [
        "How is the checkout service doing? Give me its version.",
        "How is the billing service doing?",
    ] {
        println!("--- {question}");
        match drive_once(&a2a, question).await {
            Ok(true) => used_tools += 1,
            Ok(false) => {}
            Err(e) => {
                println!("    [failed] {e}");
                failures += 1;
            }
        }
        println!();
    }

    if failures > 0 {
        return Err(format!("{failures} of 2 demo questions failed").into());
    }

    // A green run with no model proves nothing about MCP: with no model there
    // are no tool calls, so the session this example exists to demonstrate is
    // never used. Saying so is the point — inferring an exercised MCP leg
    // from an exit code is exactly the substitution this repository keeps
    // removing, and the A2A half really is fully exercised either way.
    if used_tools == 0 {
        println!("A2A leg: EXERCISED — both questions round-tripped as tasks.");
        println!("MCP leg: NOT EXERCISED — no model was reachable, so the agent");
        println!("  never called a tool and every answer above is the labelled");
        println!("  mechanical fallback. Point OPENAI_BASE_URL at a tool-capable");
        println!("  model to exercise it; see README.md.");
        return Ok(());
    }

    println!("Demo complete: {used_tools} of 2 answers used MCP tools, both over A2A.");
    Ok(())
}

/// Sends one question over A2A, prints the artifacts, and reports whether the
/// agent used its MCP tools.
///
/// The signal is the *presence* of the `tool-trace` artifact rather than a
/// count of its lines: a tool whose output spans lines would inflate a line
/// count, and presence is exactly what the caller needs to know.
async fn drive_once(
    client: &a2a_protocol_client::A2aClient,
    question: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    use a2a_protocol_types::message::Message;
    use a2a_protocol_types::params::MessageSendParams;
    use a2a_protocol_types::responses::SendMessageResponse;

    let params = MessageSendParams::new(Message::user_text(
        format!("m-{}", uuid::Uuid::new_v4()),
        question,
    ));

    match client.send_message(params).await? {
        SendMessageResponse::Task(task) => {
            println!("    state: {:?}", task.status.state);
            let mut traced = false;
            for artifact in task.artifacts.as_deref().unwrap_or_default() {
                println!("    [{}]", artifact.id.0);
                traced |= artifact.id.0 == "tool-trace";
                for part in &artifact.parts {
                    if let PartContent::Text(text) = &part.content {
                        for line in text.lines() {
                            println!("      {line}");
                        }
                    }
                }
            }
            Ok(traced)
        }
        other => Err(format!("expected a task, got {other:?}").into()),
    }
}
