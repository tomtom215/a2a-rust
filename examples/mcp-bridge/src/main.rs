// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Example: exposing a remote A2A agent as an MCP server.
//!
//! The reverse of `examples/mcp-agent`. That one is an agent that *uses* MCP
//! tools; this one makes an A2A agent *be* one, so any MCP client — an
//! editor, a coding agent, anything speaking the protocol — can call a
//! remote A2A agent without knowing A2A exists.
//!
//! ```text
//!   MCP client ──MCP over stdio──→ this bridge ──A2A JSON-RPC──→ remote agent
//!              ←──tool result─────            ←──task───────────
//! ```
//!
//! # Running it
//!
//! The bridge speaks MCP on its own stdin and stdout, which is how MCP
//! clients start servers. It takes the A2A agent's base URL as its one
//! argument, or in `A2A_AGENT_URL`:
//!
//! ```bash
//! a2a-mcp-bridge http://127.0.0.1:8080
//! ```
//!
//! In an MCP client's server list that is a command and an argument, the
//! same as any other server. One bridge process fronts one agent; three
//! agents are three entries.
//!
//! To see the whole thing work without wiring up a client, `bridge-demo`
//! stands up a sample A2A agent, spawns this binary against it, and drives
//! it as an MCP client:
//!
//! ```bash
//! cargo run -p a2a-mcp-bridge --bin bridge-demo
//! ```
//!
//! # What crosses, and what does not
//!
//! [`mapping`] holds the translation and the honest limits: A2A skills carry
//! no argument schema, so every published tool takes one string; A2A has no
//! skill selector, so the choice travels as metadata the agent may ignore;
//! and `input-required` is reported rather than bridged. Each is stated
//! where it happens.

use rmcp::ServiceExt;

mod bridge;
mod mapping;

#[cfg(test)]
mod tests;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("A2A_AGENT_URL").ok())
        .ok_or("usage: a2a-mcp-bridge <A2A agent base URL>  (or set A2A_AGENT_URL)")?;

    // Discovery before serving. An MCP server that came up and published an
    // empty tool list because the agent was briefly down is indistinguishable,
    // to its caller, from an agent with nothing to offer.
    let bridge = bridge::A2aBridge::connect(&url).await?;

    // stderr, never stdout: stdout is the MCP wire, and one stray line on it
    // desynchronizes every frame that follows.
    eprintln!(
        "a2a-mcp-bridge: '{}' at {url}, {} tool(s): {}",
        bridge.agent_name(),
        bridge.tools().len(),
        bridge
            .tools()
            .iter()
            .map(|t| t.name.as_ref())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let service = bridge.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
