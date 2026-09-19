// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The MCP server that fronts one remote A2A agent.
//!
//! One bridge process fronts one agent, because that is how MCP clients are
//! configured anyway — a list of servers, each its own command. Fronting
//! three agents means three entries, not one bridge with a routing table.
//!
//! # How an A2A task becomes an MCP call
//!
//! The two protocols agree more than they disagree, which is why this is
//! worth doing:
//!
//! | A2A | MCP |
//! |---|---|
//! | `Completed` | a tool result |
//! | `Failed`, `Rejected`, `Canceled` | a tool result with `isError` |
//! | long-running task | SEP-2663 task, polled by the client with `tasks/get` |
//! | `CancelTask` | `tasks/cancel` |
//! | `InputRequired` | *not bridged* — see [`crate::mapping::task_to_result`] |
//!
//! When the MCP client declares the tasks extension, a call becomes an MCP
//! task and the caller polls it. When it does not, the bridge blocks until
//! the A2A task settles and returns the result directly. Both paths run the
//! same driver, so they cannot drift.
//!
//! # Why it polls A2A rather than streaming it
//!
//! A2A has `SendStreamingMessage`, and it would give finer-grained progress.
//! Two reasons this uses `returnImmediately` plus `GetTask` instead. An agent
//! card may not advertise streaming at all, and a bridge that worked only
//! against streaming agents would be a bridge with a footnote. And MCP's own
//! task model *is* polling — the client drives `tasks/get` at its own
//! interval — so a poll-to-poll bridge has one clock rather than translating
//! between two. The cost is that progress updates land at poll granularity,
//! which is stated here rather than discovered.
//!
//! # A2A has no skill selector
//!
//! The bridge publishes one MCP tool per advertised skill, because the
//! descriptions are what tell a caller's model which to use. But
//! `SendMessage` has no field naming a skill: every tool sends to the same
//! agent, and the choice is advisory. The chosen skill travels in the
//! message metadata under [`SKILL_METADATA_KEY`] so an agent that wants to
//! route on it can, and an agent that does not is unaffected.

use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_client::A2aClient;
use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::{MessageSendParams, SendMessageConfiguration};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{Task, TaskState};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, CancelTaskParams, GetTaskParams,
    GetTaskResult, Implementation, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    ServerConfig, Tool, UpdateTaskParams,
};
use rmcp::service::RequestContext;
use rmcp::task_manager::{TaskContext, TaskExit, TaskManager, TaskOptions};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};

use crate::mapping::{self, MESSAGE_ARG};

/// Message-metadata key carrying the MCP tool the caller chose.
pub const SKILL_METADATA_KEY: &str = "a2a-mcp-bridge/skill";

/// How often the bridge asks the A2A agent whether the task has settled.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How long one bridged call may take before the bridge gives up on it.
///
/// A bound, not a guess at how long agents take. Without it an A2A agent
/// wedged in `Working` holds an MCP call — and on the non-task path, the
/// caller's whole request — open indefinitely. The A2A task is cancelled on
/// the way out so the agent is not left running for a caller that has gone.
const MAX_WAIT: Duration = Duration::from_secs(300);

/// An MCP server that forwards tool calls to one A2A agent.
pub struct A2aBridge {
    client: Arc<A2aClient>,
    card: AgentCard,
    tools: Vec<Tool>,
    tasks: TaskManager,
}

/// Why the bridge could not be built.
#[derive(Debug)]
pub enum BridgeError {
    /// The agent card could not be fetched or parsed.
    Discovery(String),
    /// A client could not be built for the discovered card.
    Client(String),
    /// The card's skills do not map onto MCP tools.
    Mapping(mapping::MappingError),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discovery(why) => write!(f, "agent card discovery: {why}"),
            Self::Client(why) => write!(f, "A2A client: {why}"),
            Self::Mapping(e) => write!(f, "card does not map to MCP tools: {e}"),
        }
    }
}

impl std::error::Error for BridgeError {}

impl A2aBridge {
    /// Fetches `base_url`'s agent card and builds the tool list from it.
    ///
    /// Discovery happens once, at startup, and a failure here refuses to
    /// start: an MCP server that published an empty tool list because the
    /// agent was briefly down would look to its caller exactly like an agent
    /// with nothing to offer.
    ///
    /// # Errors
    ///
    /// See [`BridgeError`].
    pub async fn connect(base_url: &str) -> Result<Self, BridgeError> {
        let card = a2a_protocol_client::resolve_agent_card(base_url)
            .await
            .map_err(|e| BridgeError::Discovery(e.to_string()))?;
        let tools = mapping::tools_from_card(&card).map_err(BridgeError::Mapping)?;
        let client = a2a_protocol_client::ClientBuilder::from_card(&card)
            .map_err(|e| BridgeError::Client(e.to_string()))?
            .build()
            .map_err(|e| BridgeError::Client(e.to_string()))?;

        Ok(Self {
            client: Arc::new(client),
            card,
            tools,
            tasks: TaskManager::new(),
        })
    }

    /// The tools this bridge publishes, in card order.
    #[must_use]
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// The name of the agent behind this bridge.
    #[must_use]
    pub fn agent_name(&self) -> &str {
        &self.card.name
    }
}

/// Runs one A2A task to settlement and renders it as an MCP result.
///
/// `progress` is present when the call was materialized as an MCP task; it
/// carries status text back to the caller and is how a `tasks/cancel`
/// reaches the A2A agent as a `CancelTask`.
async fn drive(
    client: &A2aClient,
    tool: &str,
    message: String,
    progress: Option<&TaskContext>,
) -> Result<CallToolResult, String> {
    let params = MessageSendParams::new(Message::user_text(
        format!("mcp-bridge-{}", uuid::Uuid::new_v4()),
        message,
    ))
    // `return_immediately` so the task id is in hand before the agent
    // finishes. Without it a cancel arriving mid-call would have nothing
    // to cancel.
    .with_configuration(SendMessageConfiguration {
        return_immediately: Some(true),
        ..SendMessageConfiguration::default()
    })
    .with_metadata(serde_json::json!({ SKILL_METADATA_KEY: tool }));

    let task = match client.send_message(params).await {
        Ok(SendMessageResponse::Task(task)) => task,
        // An agent may answer a message with a message rather than a task —
        // legal in A2A, and there is nothing to poll.
        Ok(SendMessageResponse::Message(reply)) => {
            return Ok(message_result(&reply));
        }
        Ok(other) => return Err(format!("unexpected A2A response: {other:?}")),
        Err(e) => return Err(format!("A2A send failed: {e}")),
    };

    let settled = wait_for_settlement(client, task, progress).await?;
    let (content, is_error) = mapping::task_to_result(&settled);
    let dropped = mapping::dropped_part_count(&settled);

    let mut result = if is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    };
    if dropped > 0 {
        // Stated rather than silent: the caller can tell that the agent
        // produced more than it is seeing.
        result.content.push(rmcp::model::ContentBlock::text(format!(
            "({dropped} non-text artifact part(s) omitted — this bridge forwards text only)"
        )));
    }
    Ok(result)
}

/// Polls until the task reaches a state MCP can report, or the bound expires.
async fn wait_for_settlement(
    client: &A2aClient,
    first: Task,
    progress: Option<&TaskContext>,
) -> Result<Task, String> {
    let deadline = Instant::now() + MAX_WAIT;
    let id = first.id.clone();
    let mut task = first;
    let mut last_note = String::new();

    loop {
        // `is_terminal` covers completed/failed/canceled/rejected;
        // input-required and auth-required are reportable too, because the
        // agent has stopped and is waiting for something this bridge cannot
        // supply. Either way there is nothing further to poll for.
        if task.status.state.is_terminal()
            || matches!(
                task.status.state,
                TaskState::InputRequired | TaskState::AuthRequired
            )
        {
            return Ok(task);
        }

        if let Some(ctx) = progress {
            if ctx.is_cancel_requested() {
                // The MCP caller gave up. Tell the agent, then report
                // whatever state the cancel left the task in rather than
                // assuming it took.
                let cancelled = client.cancel_task(id.0.clone()).await;
                return cancelled.map_err(|e| format!("A2A cancel failed: {e}"));
            }
            let note = status_note(&task);
            if note != last_note {
                ctx.set_status_message(note.clone());
                last_note = note;
            }
        }

        if Instant::now() >= deadline {
            let _ = client.cancel_task(id.0.clone()).await;
            return Err(format!(
                "the agent did not settle task '{}' within {}s; it was cancelled",
                id.0,
                MAX_WAIT.as_secs()
            ));
        }

        tokio::time::sleep(POLL_INTERVAL).await;
        task = client
            .get_task(a2a_protocol_types::params::TaskQueryParams {
                tenant: None,
                id: id.0.clone(),
                history_length: None,
            })
            .await
            .map_err(|e| format!("A2A get_task failed: {e}"))?;
    }
}

/// One line of progress for the MCP caller, from the task's own status.
fn status_note(task: &Task) -> String {
    let state = format!("{:?}", task.status.state);
    task.status
        .message
        .as_ref()
        .and_then(|m| {
            m.parts.iter().find_map(|p| match &p.content {
                a2a_protocol_types::message::PartContent::Text(text) => Some(text.clone()),
                _ => None,
            })
        })
        .map_or(state.clone(), |note| format!("{state}: {note}"))
}

/// Renders an immediate `Message` reply, which carries no task to poll.
fn message_result(reply: &a2a_protocol_types::message::Message) -> CallToolResult {
    let blocks: Vec<rmcp::model::ContentBlock> = reply
        .parts
        .iter()
        .filter_map(|p| match &p.content {
            a2a_protocol_types::message::PartContent::Text(text) => {
                Some(rmcp::model::ContentBlock::text(text.clone()))
            }
            _ => None,
        })
        .collect();
    CallToolResult::success(blocks)
}

impl ServerHandler for A2aBridge {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            // The tasks extension is what lets a long-running agent be
            // polled rather than blocking the caller's request. Declared
            // here; clients that do not ask for it get the blocking path.
            .enable_tasks()
            .build();
        info.server_info = Implementation::new(
            format!("a2a-mcp-bridge ({})", self.card.name),
            env!("CARGO_PKG_VERSION"),
        );
        info.instructions = Some(format!(
            "Tools on this server are skills of the remote A2A agent '{}': {}. \
             Each call sends one message and returns when the agent's task settles.",
            self.card.name, self.card.description
        ));
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        // The whole list fits in one page; the card was read once at startup
        // and does not grow.
        Ok(ListToolsResult {
            tools: self.tools.clone(),
            ..ListToolsResult::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let tool = request.name.to_string();
        if !self.tools.iter().any(|t| t.name == request.name) {
            return Err(McpError::invalid_params(
                format!("no tool named '{tool}' on this bridge"),
                None,
            ));
        }

        let message = request
            .arguments
            .as_ref()
            .and_then(|args| args.get(MESSAGE_ARG))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                McpError::invalid_params(
                    format!("'{tool}' needs a string argument '{MESSAGE_ARG}'"),
                    None,
                )
            })?
            .to_owned();

        let client = Arc::clone(&self.client);

        // A caller that declared the tasks extension gets a task it can poll
        // and cancel. One that did not gets the same work, awaited inline.
        let wants_task = context
            .client_capabilities()
            .is_some_and(|caps| caps.supports_tasks());
        if !wants_task {
            let result = drive(&client, &tool, message, None)
                .await
                .map_err(|why| McpError::internal_error(why, None))?;
            return Ok(CallToolResponse::Complete(result));
        }

        let spawned = self.tasks.spawn(
            TaskOptions::default().with_status_message("contacting the A2A agent"),
            move |ctx| {
                Box::pin(async move {
                    drive(&client, &tool, message, Some(&ctx))
                        .await
                        .map_err(|why| TaskExit::Error(McpError::internal_error(why, None)))
                })
            },
        );
        Ok(CallToolResponse::Task(rmcp::model::CreateTaskResult::new(
            spawned,
        )))
    }

    async fn get_task(
        &self,
        request: GetTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, McpError> {
        self.tasks
            .get_task(&request.task_id)
            .map(GetTaskResult::new)
    }

    async fn update_task(
        &self,
        request: UpdateTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        // Accepted for protocol completeness. Nothing in this bridge requests
        // input mid-task, because A2A's own `input-required` is not bridged
        // (see `crate::mapping::task_to_result`), so there is never an
        // outstanding key for a response to answer.
        self.tasks
            .update_task(&request.task_id, request.input_responses)
    }

    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        // Records the intent; `wait_for_settlement` observes it on its next
        // poll and issues the A2A `CancelTask`. Cooperative on both sides,
        // which is what both specs ask for.
        self.tasks.cancel_task(&request.task_id)
    }
}
