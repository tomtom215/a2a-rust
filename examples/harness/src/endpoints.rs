// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Where a surface-run agent is listening, and the interfaces its card must
//! advertise.
//!
//! # Why this is here rather than in each example
//!
//! `Endpoints` and [`interfaces`] were byte-identical in `genai-agent`,
//! `rig-agent` and `multi-lang-team` — three copies of the function that
//! decides what an agent *claims* to serve. This crate already exists because
//! a duplicated scorer eventually disagrees with itself; a duplicated card
//! builder is the same shape with a worse failure, because the disagreement is
//! published in the agent card rather than printed in a report.
//!
//! # Why the binding strings are not written out here
//!
//! They were, in all three copies. The SDK ships
//! [`AgentInterface::jsonrpc`], [`AgentInterface::rest`] and
//! [`AgentInterface::grpc`] precisely so nobody types `"HTTP+JSON"` by hand —
//! its own documentation says a typo there "is a card that lies". Using the
//! constructors means the three spec-named bindings cannot be misspelled at
//! all.
//!
//! `WEBSOCKET` has no constructor because it is not a spec binding: it is a
//! §12 custom one this SDK adds. It is spelled out below, once, with that
//! reason attached rather than left looking like an oversight.

use a2a_protocol_types::agent_card::AgentInterface;

use crate::Binding;

/// Where the agent is listening, per binding.
pub struct Endpoints {
    /// JSON-RPC (§9). Shares a socket with REST.
    pub jsonrpc: String,
    /// HTTP+JSON (§11). Shares a socket with JSON-RPC.
    pub rest: String,
    /// gRPC (§10), as `host:port`.
    pub grpc: String,
    /// WebSocket (§12 custom), as a `ws://` URL.
    pub websocket: String,
}

impl Endpoints {
    /// The URL this agent serves `binding` on.
    ///
    /// Exhaustive over [`Binding`] on purpose: adding a binding to the matrix
    /// without giving it an address stops compiling here, rather than
    /// producing a card that advertises three of four.
    #[must_use]
    pub fn url_for(&self, binding: Binding) -> &str {
        match binding {
            Binding::JsonRpc => &self.jsonrpc,
            Binding::HttpJson => &self.rest,
            Binding::Grpc => &self.grpc,
            Binding::WebSocket => &self.websocket,
        }
    }
}

/// The four interfaces the card must advertise, one per [`Binding`].
#[must_use]
pub fn interfaces(ep: &Endpoints) -> Vec<AgentInterface> {
    Binding::ALL
        .iter()
        .map(|b| {
            let url = ep.url_for(*b);
            match b {
                Binding::JsonRpc => AgentInterface::jsonrpc(url),
                Binding::HttpJson => AgentInterface::rest(url),
                Binding::Grpc => AgentInterface::grpc(url),
                // §12 custom binding — the SDK offers no constructor for it
                // because the specification does not name it.
                Binding::WebSocket => AgentInterface::new(url, "WEBSOCKET"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
