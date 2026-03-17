// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Agent card HTTP handlers (static, dynamic, and caching utilities).

pub mod caching;
pub mod dynamic_handler;
pub mod hot_reload;
pub mod static_handler;

pub use dynamic_handler::{AgentCardProducer, DynamicAgentCardHandler};
pub use hot_reload::HotReloadAgentCardHandler;
pub use static_handler::StaticAgentCardHandler;

/// CORS `Access-Control-Allow-Origin` header value for public agent cards.
pub const CORS_ALLOW_ALL: &str = "*";
