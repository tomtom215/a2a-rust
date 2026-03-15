// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Agent card HTTP handlers (static and dynamic).

pub mod dynamic_handler;
pub mod static_handler;

pub use dynamic_handler::{AgentCardProducer, DynamicAgentCardHandler};
pub use static_handler::StaticAgentCardHandler;

/// CORS `Access-Control-Allow-Origin` header value for public agent cards.
pub const CORS_ALLOW_ALL: &str = "*";
