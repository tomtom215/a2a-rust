// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The MCP server the agent talks to — a separate process, speaking MCP over
//! its own stdin and stdout.
//!
//! It is a *whole separate binary* rather than a module the agent calls, and
//! that is the point of the example. The agent has no compiled-in knowledge of
//! these tools: it spawns this program, asks it what it can do, and works with
//! whatever comes back. Replace this binary with any other MCP server —
//! someone else's, in any language — and the agent is unchanged.
//!
//! Run it by hand to see the wire:
//!
//! ```bash
//! echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{
//!   "protocolVersion":"2025-06-18","capabilities":{},
//!   "clientInfo":{"name":"curl","version":"0"}}}' \
//!   | cargo run -q -p mcp-a2a-agent --bin mcp-tool-server
//! ```

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, ServiceExt, tool, tool_router};
use serde::Deserialize;

/// One row of the inventory the tools read.
///
/// The agent never sees this type, or the table below. It learns the *shape*
/// of an answer from the JSON the tools return, which is all MCP gives it and
/// all it needs.
struct Service {
    name: &'static str,
    state: &'static str,
    version: &'static str,
    uptime_s: u64,
}

/// The whole backing store. A real server would reach a database or a
/// monitoring API; a constant keeps the example deterministic.
///
/// The names match `examples/incident-response`'s runbooks and
/// `examples/rig-agent`'s in-process catalogue, so all three describe the same
/// imaginary system.
const INVENTORY: &[Service] = &[
    Service {
        name: "payments-api",
        state: "degraded",
        version: "4.2.1",
        uptime_s: 1_814_400,
    },
    Service {
        name: "checkout",
        state: "healthy",
        version: "2.8.0",
        uptime_s: 259_200,
    },
    Service {
        name: "search",
        state: "healthy",
        version: "1.19.3",
        uptime_s: 7_776_000,
    },
];

/// Arguments for [`ToolServer::service_status`].
///
/// `JsonSchema` is what produces the tool's `inputSchema` on the wire, so this
/// struct *is* the published contract — there is no second place to keep in
/// step with it, which is the failure mode a hand-written schema invites.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ServiceStatusArgs {
    /// Exact service name, as returned by `list_services`.
    service: String,
}

#[derive(Clone)]
struct ToolServer;

#[tool_router(server_handler)]
impl ToolServer {
    /// List the name of every service in the inventory. Call this first when
    /// the caller names a service you do not recognise.
    #[tool]
    fn list_services(&self) -> String {
        let names: Vec<&str> = INVENTORY.iter().map(|service| service.name).collect();
        serde_json::Value::from(names).to_string()
    }

    /// Look up one service's current state, deployed version, and uptime in
    /// seconds.
    #[tool]
    fn service_status(
        &self,
        Parameters(args): Parameters<ServiceStatusArgs>,
    ) -> Result<String, ErrorData> {
        // An unknown name is an error *for this call*, reported through MCP's
        // own error channel. The agent turns it back into something the model
        // reads, rather than failing the A2A task — see `crate::agent`. The
        // message names the recovery tool because the model is its reader.
        let found = INVENTORY
            .iter()
            .find(|service| service.name == args.service)
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    format!(
                        "no service named '{}'; call list_services for the inventory",
                        args.service
                    ),
                    None,
                )
            })?;
        Ok(serde_json::json!({
            "service": found.name,
            "state": found.state,
            "version": found.version,
            "uptimeSeconds": found.uptime_s,
        })
        .to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // stdout is the MCP wire. Anything else written there corrupts the
    // protocol, which is why this program logs nothing: a stray `println!` in
    // an MCP stdio server is the classic way to break one.
    let service = ToolServer.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
