// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The MCP side: connect to a server, learn what it offers, call it.
//!
//! This module is the whole bridge. Above it, [`crate::agent`]'s loop deals in
//! `rig_core` types and never mentions MCP; below it, the server deals in MCP
//! and never mentions rig or A2A. The translation is three things — a
//! `Tool` becomes a `ToolDefinition`, a `serde_json::Value` of arguments
//! becomes a `CallToolRequestParams`, and a `CallToolResult` becomes the text
//! the model reads.
//!
//! # Nothing here knows what the tools are
//!
//! There is no catalogue in this crate. [`McpTools::connect`] asks the server
//! and keeps the answer. Point the agent at a different MCP server — someone
//! else's, in any language — and it works with that server's tools instead,
//! with no change here. That is the difference between this example and
//! `examples/rig-agent`, whose two tools are compiled in.
//!
//! # Two kinds of failure, and they are not the same
//!
//! This is the distinction worth copying, because conflating them is what
//! makes an agent either brittle or dishonest:
//!
//! * **The tool ran and refused.** An unknown service, a bad argument. MCP
//!   reports it as a JSON-RPC error ([`ServiceError::McpError`]) or as a
//!   result carrying `isError`. Either way it is *information for the model*,
//!   which can read it and try something else — so it comes back as
//!   [`ToolOutcome::Refused`] and the A2A task stays alive.
//! * **The server is gone.** The child process died, the transport closed, the
//!   call timed out. No amount of re-prompting fixes that, and an answer
//!   produced without the tools the agent advertised would be a guess wearing
//!   a researched answer's clothes. That is [`McpError::Transport`], and
//!   [`crate::agent`] fails the A2A task with it.

use std::sync::Arc;

use rig_core::completion::ToolDefinition;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::service::{RoleClient, RunningService, ServiceError};
use rmcp::{ServiceExt, transport::IntoTransport};
use serde_json::Value;

/// A live MCP session, plus the catalogue it reported at connect time.
pub struct McpTools {
    service: RunningService<RoleClient, ()>,
    catalogue: Vec<ToolDefinition>,
}

/// Why the MCP session could not be established or used.
///
/// Only infrastructure lives here. A tool that ran and refused is a
/// [`ToolOutcome::Refused`], not an error — see the module docs.
#[derive(Debug)]
pub enum McpError {
    /// The server could not be reached, or the session broke.
    Transport(String),
    /// The server connected but offered no tools, which makes the agent a
    /// plain completion bot wearing a tool-using agent's agent card. Better to
    /// refuse at startup than to answer questions it cannot research.
    NoTools,
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(why) => write!(f, "MCP transport: {why}"),
            Self::NoTools => write!(f, "the MCP server offered no tools"),
        }
    }
}

impl std::error::Error for McpError {}

/// What one `tools/call` produced.
#[derive(Debug, PartialEq, Eq)]
pub enum ToolOutcome {
    /// The tool ran. Its text output, concatenated in order.
    Output(String),
    /// The tool ran and refused, with the reason it gave. Goes to the model.
    Refused(String),
}

impl McpTools {
    /// Connects over `transport`, initializes the session, and lists the
    /// server's tools.
    ///
    /// Generic over the transport so the runnable path can spawn a child
    /// process over stdio — what a real MCP client does — while tests use an
    /// in-process duplex pair and exercise the same code.
    ///
    /// # Errors
    ///
    /// [`McpError::Transport`] if the session cannot be established or the
    /// listing fails, and [`McpError::NoTools`] if the server offers none.
    pub async fn connect<T, E, A>(transport: T) -> Result<Self, McpError>
    where
        T: IntoTransport<RoleClient, E, A>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let service = ().serve(transport).await.map_err(|e| McpError::Transport(e.to_string()))?;

        // `list_all_tools` walks the cursor for us; a server that paginates
        // its catalogue would otherwise be silently truncated to page one.
        let discovered = service
            .list_all_tools()
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;
        if discovered.is_empty() {
            return Err(McpError::NoTools);
        }

        let catalogue = discovered.into_iter().map(as_rig_definition).collect();
        Ok(Self { service, catalogue })
    }

    /// The discovered catalogue, in rig's shape, ready to send to a model.
    pub fn definitions(&self) -> &[ToolDefinition] {
        &self.catalogue
    }

    /// The discovered tool names, for the agent card and the startup banner.
    pub fn names(&self) -> Vec<&str> {
        self.catalogue
            .iter()
            .map(|tool| tool.name.as_str())
            .collect()
    }

    /// Calls one tool and returns what the model should read.
    ///
    /// # Errors
    ///
    /// [`McpError::Transport`] only when the session itself failed. A tool
    /// that ran and refused returns `Ok(`[`ToolOutcome::Refused`]`)`.
    pub async fn invoke(&self, name: &str, arguments: &Value) -> Result<ToolOutcome, McpError> {
        let mut params = CallToolRequestParams::new(name.to_owned());
        // A model may send `null` or a non-object for a no-argument tool.
        // MCP's `arguments` is an object or absent, so anything else is
        // absent rather than an error: the server's own schema is the
        // authority on whether that is acceptable, not this bridge.
        if let Some(object) = arguments.as_object() {
            params = params.with_arguments(object.clone());
        }

        match self.service.call_tool(params).await {
            Ok(result) => Ok(read_result(&result)),
            // The server answered, and the answer was "no". That is the
            // tool's verdict, not a broken session — hand it to the model.
            Err(ServiceError::McpError(data)) => Ok(ToolOutcome::Refused(data.message.to_string())),
            Err(other) => Err(McpError::Transport(other.to_string())),
        }
    }
}

// No explicit shutdown, deliberately. Dropping the session drops the
// transport with it, and on the child-process transport rmcp's own
// `ChildWithCleanup::drop` kills the server rather than leaving a zombie
// (`rmcp-3.4.0/src/transport/child_process.rs`, the `Drop` impl, which calls
// `kill()` and says why it is not `start_kill()`). A second teardown
// mechanism layered on top would only obscure which one is responsible.

/// Translates one MCP tool into the definition a rig model is sent.
///
/// `description` is optional in MCP and required by rig. An empty one is
/// passed through rather than invented: what the model is told about a tool is
/// the server's claim to make, and a plausible-sounding substitute written
/// here would be this bridge putting words in its mouth.
fn as_rig_definition(tool: rmcp::model::Tool) -> ToolDefinition {
    ToolDefinition {
        name: tool.name.to_string(),
        description: tool.description.unwrap_or_default().to_string(),
        parameters: Value::Object(Arc::unwrap_or_clone(tool.input_schema)),
    }
}

/// Reads a successful `tools/call` response into text for the model.
fn read_result(result: &CallToolResult) -> ToolOutcome {
    let text = result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            // Images, audio and embedded resources are legal MCP results and
            // meaningless to a text completion. Dropping them is honest;
            // stringifying them would feed the model noise it would then
            // quote back.
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    // `isError` is the tool saying it failed while still returning a result,
    // which is a refusal rather than a transport fault.
    if result.is_error == Some(true) {
        ToolOutcome::Refused(text)
    } else {
        ToolOutcome::Output(text)
    }
}
