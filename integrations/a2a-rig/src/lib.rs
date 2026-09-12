// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Serve a [rig](https://github.com/0xPlaygrounds/rig) agent over the A2A
//! protocol.
//!
//! [`RigExecutor`] wraps any `rig_core::completion::CompletionModel` as an
//! [`a2a_protocol_server::AgentExecutor`], so a rig model answers A2A
//! `message/send` and `message/stream` calls over JSON-RPC, REST, WebSocket or
//! gRPC without the caller writing any protocol code.
//!
//! ```no_run
//! use a2a_protocol_server::builder::RequestHandlerBuilder;
//! use a2a_protocol_server::dispatch::JsonRpcDispatcher;
//! use a2a_rig::{RigExecutor, agent_card};
//! use rig_core::client::CompletionClient;
//! use rig_core::providers::openai;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = openai::CompletionsClient::builder()
//!     .api_key(&std::env::var("OPENAI_API_KEY")?)
//!     .build()?;
//!
//! let handler = RequestHandlerBuilder::new(
//!     RigExecutor::new(client.completion_model("gpt-4o-mini"))
//!         .with_preamble("You are a helpful assistant."),
//! )
//! .with_agent_card(agent_card("https://agent.example.com", "gpt-4o-mini").build())
//! .build()?;
//!
//! let dispatcher = JsonRpcDispatcher::new(std::sync::Arc::new(handler));
//! # let _ = dispatcher;
//! # Ok(())
//! # }
//! ```
//!
//! # What this does and does not do
//!
//! One A2A message becomes one rig completion request. There is no tool loop, no
//! conversation history, and no token-level streaming into A2A artifacts: the
//! answer is emitted as a single artifact when the completion returns. Those are
//! rig's agent-layer concerns, and a bridge that pretended to cover them would be
//! claiming fidelity it does not have.
//!
//! Cancellation is real, not nominal — see [`RigExecutor`].

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]

mod card;
mod executor;

pub use card::{AgentCardBuilder, agent_card};
pub use executor::RigExecutor;

/// Compiles the examples in `README.md`, so the quickstart cannot drift from the
/// API without the test suite saying so. Doc-test only; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
