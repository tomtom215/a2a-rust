// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An agent with tool calling from the SDK alone (ADR 0022).
//!
//! `rig-agent` and `mcp-agent` each write their own tool loop over rig-core.
//! This one writes none: `ToolLoopExecutor` runs the SDK's `ToolLoop` over
//! `OpenAiChatModel`, and the agent is two tools, a preamble and a card. It
//! serves the agent over JSON-RPC on loopback, sends it one question with
//! the A2A client, prints the answer and the `tool-trace` artifact, and
//! exits non-zero when the task did not complete.
//!
//! ```bash
//! # llama.cpp's llama-server with --jinja (README.md has the walkthrough)
//! TOOL_LOOP_BASE_URL=http://127.0.0.1:8080/v1 cargo run -p tool-loop-agent -- \
//!     "How is the billing service doing?"
//! ```
//!
//! | Variable | Default |
//! |---|---|
//! | `TOOL_LOOP_BASE_URL` | `http://127.0.0.1:8080/v1` |
//! | `TOOL_LOOP_MODEL` | `qwen3:1.7b` |
//! | `TOOL_LOOP_API_KEY` | none (local servers need none) |
//! | `TOOL_LOOP_THINKING` | unset: sends `chat_template_kwargs: {"enable_thinking": false}`; `1` sends nothing |
//! | `TOOL_LOOP_MODEL_TIMEOUT_SECS` | 120 (`OpenAiChatModel`'s default): how long one model turn may take |
//! | `TOOL_LOOP_EXTRA_BODY` | none; a JSON object merged into every request after the above (sampler, seed: `docs/tool-calling-study.md` uses it) |

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::tool_loop::{
    ANSWER_ARTIFACT_ID, OpenAiChatModel, TRACE_ARTIFACT_ID, ToolLoop, ToolLoopExecutor, ToolModel,
};

mod tools;

/// What the model is told before every turn: `examples/rig-agent`'s
/// preamble, measured there on 2026-10-20. Small models otherwise tell the
/// caller to call a tool rather than call it, and restate a tool's numbers
/// from memory.
pub const PREAMBLE: &str = "You are an operations agent with tools for checking services. \
Call the tools yourself to answer; never ask the user to call a tool. \
If a tool returns an error that suggests another tool, call that tool, then answer. \
Report values exactly as the tools returned them.";

/// The question asked when none is given: a service the inventory does not
/// have, so the model has to recover from a tool error.
pub const DEFAULT_PROMPT: &str = "How is the billing service doing?";

fn card(url: &str) -> AgentCard {
    AgentCard::new("tool-loop-agent", "0.0.0", AgentInterface::jsonrpc(url))
        .with_description("Answers questions about a service inventory by calling its tools")
        .with_skill(
            AgentSkill::new(
                "service-status",
                "Service status",
                "Looks services up with list_services and service_status",
            )
            .with_tags(["tool-calling", "operations"]),
        )
        // `ask` streams; see there for why.
        .with_capabilities(AgentCapabilities::none().with_streaming(true))
}

/// Serves `model` behind a `ToolLoopExecutor` on an ephemeral loopback port
/// and returns its URL.
pub async fn serve_agent(model: impl ToolModel) -> Result<String, Box<dyn std::error::Error>> {
    // The card names the port, so it is learned first.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = probe.local_addr()?;
    drop(probe);
    let url = format!("http://{addr}");
    let tool_loop = ToolLoop::new(model, tools::catalogue()?).with_preamble(PREAMBLE);
    let handler = Arc::new(
        RequestHandlerBuilder::new(ToolLoopExecutor::new(tool_loop))
            .with_agent_card(card(&url))
            .build()?,
    );
    serve_with_addr(addr, JsonRpcDispatcher::new(handler)).await?;
    Ok(url)
}

/// Asks the agent at `url` one question, as any A2A client would, and
/// returns the task as stored once the run ends.
///
/// Streamed rather than a blocking send. A blocking send moves no bytes
/// until the run is over, and the server closes a connection idle for
/// 75 seconds (`DEFAULT_IDLE_TIMEOUT`): on a busy CPU a two-round run of a
/// small model can take longer than that, and the caller saw a dropped
/// connection rather than an answer (3 of 15 runs on 2026-10-20; a stand-in
/// model answering after 80 s reproduced it at exactly 75 s). A stream
/// carries a keep-alive every 30 seconds.
pub async fn ask(url: &str, prompt: &str) -> Result<Task, Box<dyn std::error::Error>> {
    let client = ClientBuilder::new(url)
        .with_timeout(Duration::from_secs(60))
        .build()?;
    let mut stream = client
        .stream_message(MessageSendParams::new(Message::user_text("q1", prompt)))
        .await?;
    let mut task_id = None;
    while let Some(event) = stream.next().await {
        match event? {
            StreamResponse::Task(task) => task_id = Some(task.id),
            StreamResponse::StatusUpdate(update) => {
                task_id.get_or_insert(update.task_id);
            }
            _ => {}
        }
    }
    let id = task_id.ok_or("the stream named no task")?;
    Ok(client.get_task(TaskQueryParams::new(id.0)).await?)
}

/// The text of the artifact named `id`.
pub fn artifact_text<'a>(task: &'a Task, id: &str) -> Option<&'a str> {
    task.artifacts
        .as_deref()?
        .iter()
        .find(|a| a.id.0 == id)?
        .parts
        .iter()
        .find_map(Part::text_content)
}

fn model_from_env() -> Result<OpenAiChatModel, Box<dyn std::error::Error>> {
    let base = std::env::var("TOOL_LOOP_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_owned());
    let name = std::env::var("TOOL_LOOP_MODEL").unwrap_or_else(|_| "qwen3:1.7b".to_owned());
    let mut model = OpenAiChatModel::new(&base, name)?;
    if let Ok(key) = std::env::var("TOOL_LOOP_API_KEY") {
        model = model.with_api_key(key);
    }
    if let Ok(secs) = std::env::var("TOOL_LOOP_MODEL_TIMEOUT_SECS") {
        model = model.with_timeout(Duration::from_secs(secs.parse()?));
    }
    // Qwen3 thinks before it answers unless told not to, which on a CPU
    // multiplies each turn's time; llama-server reads this per request.
    let mut extra = serde_json::Map::new();
    if std::env::var("TOOL_LOOP_THINKING").as_deref() != Ok("1") {
        extra.insert(
            "chat_template_kwargs".to_owned(),
            serde_json::json!({"enable_thinking": false}),
        );
    }
    if let Ok(more) = std::env::var("TOOL_LOOP_EXTRA_BODY") {
        let serde_json::Value::Object(more) = serde_json::from_str(&more)? else {
            return Err("TOOL_LOOP_EXTRA_BODY must be a JSON object".into());
        };
        extra.extend(more);
    }
    if !extra.is_empty() {
        model = model.with_extra_body(extra);
    }
    Ok(model)
}

#[tokio::main]
async fn main() -> ExitCode {
    let prompt = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let prompt = if prompt.trim().is_empty() {
        DEFAULT_PROMPT.to_owned()
    } else {
        prompt
    };
    let run = async {
        let model = model_from_env()?;
        println!("model: {model:?}");
        let url = serve_agent(model).await?;
        println!("agent: {url}");
        println!("asked: {prompt}");
        ask(&url, &prompt).await
    };
    let task = match run.await {
        Ok(task) => task,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("state: {}", task.status.state);
    if let Some(answer) = artifact_text(&task, ANSWER_ARTIFACT_ID) {
        println!("[ {ANSWER_ARTIFACT_ID} ]\n{answer}");
    }
    if let Some(trace) = artifact_text(&task, TRACE_ARTIFACT_ID) {
        println!("[ {TRACE_ARTIFACT_ID} ]\n{trace}");
    }
    if task.status.state == TaskState::Completed {
        ExitCode::SUCCESS
    } else {
        if let Some(reason) = task.status.message.as_ref().and_then(Message::text) {
            eprintln!("reason: {reason}");
        }
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests;
