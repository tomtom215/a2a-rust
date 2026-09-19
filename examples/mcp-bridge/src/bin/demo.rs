// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The whole bridge, end to end, in one command.
//!
//! Stands up a sample A2A agent, spawns `a2a-mcp-bridge` against it as a real
//! child process, and then acts as an MCP client — `initialize`, `tools/list`,
//! `tools/call`, `tasks/get` — printing what crosses. Three processes' worth
//! of protocol in one binary, so the claim in the README can be checked rather
//! than believed.
//!
//! ```bash
//! cargo run -p a2a-mcp-bridge --bin bridge-demo
//! ```
//!
//! The sample agent reads the bridge's skill hint out of the A2A message
//! metadata and routes on it, which is the only way an A2A agent can act on
//! the MCP tool a caller chose — the protocol has no skill selector of its
//! own. An agent that ignores the hint still works; it just answers every
//! tool the same way.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, GetTaskParams,
    Implementation, TaskPayload, TaskStatus as McpTaskStatus,
};

/// The metadata key the bridge uses to name the chosen tool.
const SKILL_KEY: &str = "a2a-mcp-bridge/skill";

const SLOW_SKILL: &str = "slow_report";
const FAILING_SKILL: &str = "always_fails";

// ── The sample A2A agent ─────────────────────────────────────────────────────

struct SampleAgent;

impl AgentExecutor for SampleAgent {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let skill = ctx
                .metadata
                .as_ref()
                .and_then(|m| m.get(SKILL_KEY))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned();

            let asked = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text(text) => Some(text.clone()),
                    _ => None,
                })
                .ok_or_else(|| A2aError::invalid_params("message contains no text part"))?;

            if skill == FAILING_SKILL {
                return Err(A2aError::internal(
                    "this skill always fails, so the bridge has an error to map",
                ));
            }

            // Three visible steps, slow enough that an MCP client polling
            // `tasks/get` sees more than one state. Without that the task
            // path would be indistinguishable from the blocking one.
            for step in ["gathering", "correlating", "writing up"] {
                progress(queue, ctx, step).await?;
                tokio::time::sleep(Duration::from_millis(120)).await;
            }

            emit_artifact(
                queue,
                ctx,
                &format!(
                    "Report for '{asked}': 3 services checked, 1 degraded (payments-api). \
                     [answered via A2A skill '{}']",
                    if skill.is_empty() {
                        "unspecified"
                    } else {
                        &skill
                    }
                ),
            )
            .await?;
            status(queue, ctx, TaskState::Completed, None).await
        })
    }
}

async fn progress(queue: &dyn EventQueueWriter, ctx: &RequestContext, note: &str) -> A2aResult<()> {
    status(queue, ctx, TaskState::Working, Some(note)).await
}

async fn status(
    queue: &dyn EventQueueWriter,
    ctx: &RequestContext,
    state: TaskState,
    note: Option<&str>,
) -> A2aResult<()> {
    queue
        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            status: TaskStatus {
                state,
                message: note.map(|text| Message {
                    id: MessageId::new(format!("note-{text}")),
                    role: MessageRole::Agent,
                    parts: vec![Part::text(text)],
                    task_id: None,
                    context_id: None,
                    reference_task_ids: None,
                    extensions: None,
                    metadata: None,
                }),
                timestamp: None,
            },
            metadata: None,
        }))
        .await
}

async fn emit_artifact(
    queue: &dyn EventQueueWriter,
    ctx: &RequestContext,
    text: &str,
) -> A2aResult<()> {
    queue
        .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            artifact: Artifact::new("report", vec![Part::text(text)]),
            append: None,
            last_chunk: Some(true),
            metadata: None,
        }))
        .await
}

fn sample_card(url: &str) -> AgentCard {
    let skill = |id: &str, name: &str, description: &str| {
        AgentSkill::new(id, name, description).with_tags(["demo"])
    };
    AgentCard::new(
        "Sample Reporting Agent",
        env!("CARGO_PKG_VERSION"),
        AgentInterface::jsonrpc(url),
    )
    .with_description("A2A agent with two skills, used to demonstrate the MCP bridge")
    .with_input_modes(["text/plain"])
    .with_output_modes(["text/plain"])
    .with_skill(skill(
        SLOW_SKILL,
        "Slow service report",
        "Produce a service report. Takes a few hundred milliseconds.",
    ))
    .with_skill(skill(
        FAILING_SKILL,
        "Always fails",
        "A skill that always fails, so an error has something to cross.",
    ))
    .with_capabilities(AgentCapabilities::none().with_streaming(true))
}

/// Starts the sample agent and returns its base URL.
async fn start_agent() -> Result<String, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let handler = Arc::new(
        RequestHandlerBuilder::new(SampleAgent)
            .with_agent_card(sample_card(&url))
            .build()?,
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
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
    Ok(url)
}

// ── The demo ─────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("A2A → MCP bridge demo");
    println!("=====================\n");

    let agent_url = start_agent().await?;
    println!("1. Sample A2A agent listening on {agent_url}");

    let bridge_bin = std::env::current_exe()?
        .parent()
        .ok_or("the running binary has no parent directory")?
        .join(if cfg!(windows) {
            "a2a-mcp-bridge.exe"
        } else {
            "a2a-mcp-bridge"
        });
    let mut command = tokio::process::Command::new(&bridge_bin);
    command.arg(&agent_url);
    let transport = rmcp::transport::TokioChildProcess::new(command)
        .map_err(|e| format!("could not spawn {}: {e}", bridge_bin.display()))?;
    println!("2. Spawned the bridge: {}", bridge_bin.display());

    // A tasks-capable MCP client, so long-running A2A tasks come back as MCP
    // tasks to poll rather than blocking this request.
    let client = ClientConfig::new(
        ClientCapabilities::builder().enable_tasks().build(),
        Implementation::new("bridge-demo", env!("CARGO_PKG_VERSION")),
    )
    .serve(transport)
    .await?;
    println!("3. MCP session up, tasks extension declared\n");

    let tools = client.list_all_tools().await?;
    println!("Tools the bridge published from the agent card:");
    for tool in &tools {
        println!(
            "  {} — {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }
    println!();

    let mut failures = 0_u8;
    for (tool, expectation) in [
        (SLOW_SKILL, "completes after a few polls"),
        (FAILING_SKILL, "comes back as an MCP error result"),
    ] {
        println!("--- calling '{tool}' ({expectation})");
        match call_and_settle(&client, tool).await {
            Ok(result) => print_result(&result),
            Err(e) => {
                println!("    [failed] {e}");
                failures += 1;
            }
        }
        println!();
    }

    if failures == 0 {
        println!("Demo complete. A2A tasks crossed to MCP as tasks, and back as results.");
        Ok(())
    } else {
        Err(format!("{failures} of 2 bridged calls failed").into())
    }
}

/// Calls one bridged tool and, if it came back as a task, polls it out.
async fn call_and_settle(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ClientConfig>,
    tool: &str,
) -> Result<CallToolResult, Box<dyn std::error::Error>> {
    let params = CallToolRequestParams::new(tool.to_owned()).with_arguments(
        serde_json::json!({ "message": "how are the payment services?" })
            .as_object()
            .ok_or("an object literal")?
            .clone(),
    );

    // `call_tool` drives the task to completion for us; `call_tool_once`
    // returns the raw first response, which is what makes the task
    // materialization visible instead of hidden behind a helper.
    let first = client.call_tool_once(params).await?;
    let created = match first {
        rmcp::model::CallToolResponse::Complete(result) => {
            println!("    the bridge answered inline (no task)");
            return Ok(result);
        }
        rmcp::model::CallToolResponse::Task(created) => created,
        rmcp::model::CallToolResponse::InputRequired(_) => {
            return Err("the bridge asked for input, which it should never do".into());
        }
        // `CallToolResponse` is non-exhaustive: a later MCP revision may add
        // a response kind this demo predates. Refusing loudly beats guessing.
        other => return Err(format!("unhandled MCP response kind: {other:?}").into()),
    };

    let task_id = created.task.task_id.clone();
    println!("    materialized as MCP task {task_id}");

    let mut polls = 0_u32;
    loop {
        let detail = client.get_task(GetTaskParams::new(task_id.clone())).await?;
        polls += 1;
        match detail.task.payload {
            TaskPayload::Working => {
                if let Some(note) = detail.task.task.status_message.as_deref() {
                    println!("    poll {polls}: working — {note}");
                }
            }
            TaskPayload::Completed { result } => {
                println!("    settled after {polls} poll(s)");
                return Ok(serde_json::from_value(serde_json::Value::Object(result))?);
            }
            TaskPayload::Failed { error } => {
                return Err(format!("the bridge's task failed: {error:?}").into());
            }
            other => return Err(format!("unexpected MCP task payload: {other:?}").into()),
        }
        if detail.task.task.status != McpTaskStatus::Working {
            return Err("task left Working without a terminal payload".into());
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

fn print_result(result: &CallToolResult) {
    println!(
        "    isError: {}",
        result
            .is_error
            .map_or("absent", |e| if e { "true" } else { "false" })
    );
    for block in &result.content {
        if let rmcp::model::ContentBlock::Text(text) = block {
            for line in text.text.lines() {
                println!("      {line}");
            }
        }
    }
}
