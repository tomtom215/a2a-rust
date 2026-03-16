// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A protocol v1.0 — server framework.
//!
//! Provides [`RequestHandler`] and [`AgentExecutor`] for implementing A2A
//! agents over HTTP/1.1 and HTTP/2 using hyper 1.x.
//!
//! # Quick start
//!
//! 1. Implement [`AgentExecutor`] with your agent logic.
//! 2. Build a [`RequestHandler`] via [`RequestHandlerBuilder`].
//! 3. Wire [`JsonRpcDispatcher`] or [`RestDispatcher`] into your hyper server.
//!
//! # Module overview
//!
//! | Module | Contents |
//! |---|---|
//! | [`error`] | [`ServerError`], [`ServerResult`] |
//! | [`executor`] | [`AgentExecutor`] trait |
//! | [`handler`] | [`RequestHandler`], [`SendMessageResult`], [`HandlerLimits`] |
//! | [`builder`] | [`RequestHandlerBuilder`] |
//! | [`store`] | [`TaskStore`], [`InMemoryTaskStore`] |
//! | [`streaming`] | Event queues, SSE response builder |
//! | [`push`] | Push config store, push sender |
//! | [`agent_card`] | Static/dynamic agent card handlers |
//! | [`dispatch`] | [`JsonRpcDispatcher`], [`RestDispatcher`] |
//! | [`interceptor`] | [`ServerInterceptor`], [`ServerInterceptorChain`] |
//! | [`request_context`] | [`RequestContext`] |
//! | [`call_context`] | [`CallContext`] (includes HTTP headers for auth) |
//! | [`metrics`] | [`Metrics`] trait (request counts, latency, errors) |
//!
//! # Transport limitations
//!
//! The A2A specification lists gRPC as an optional transport binding. This
//! implementation currently supports **HTTP + SSE only**; gRPC transport is
//! not yet implemented.
//!
//! # Rate limiting
//!
//! This library does **not** perform request rate limiting. Deployments that
//! require rate limiting should handle it in a reverse proxy (e.g. nginx,
//! Envoy) or a middleware layer in front of the A2A handler.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

#[macro_use]
mod trace;

pub mod agent_card;
pub mod builder;
pub mod call_context;
pub mod dispatch;
pub mod error;
pub mod executor;
pub mod handler;
pub mod interceptor;
pub mod metrics;
pub mod push;
pub mod request_context;
pub mod store;
pub mod streaming;

// ── Flat re-exports ───────────────────────────────────────────────────────────

pub use agent_card::{
    AgentCardProducer, DynamicAgentCardHandler, StaticAgentCardHandler, CORS_ALLOW_ALL,
};
pub use builder::RequestHandlerBuilder;
pub use call_context::CallContext;
pub use dispatch::{CorsConfig, DispatchConfig, JsonRpcDispatcher, RestDispatcher};
pub use error::{ServerError, ServerResult};
pub use executor::AgentExecutor;
pub use handler::{HandlerLimits, RequestHandler, SendMessageResult};
pub use interceptor::{ServerInterceptor, ServerInterceptorChain};
pub use metrics::Metrics;
pub use push::{
    HttpPushSender, InMemoryPushConfigStore, PushConfigStore, PushRetryPolicy, PushSender,
};
pub use request_context::RequestContext;
pub use store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};
pub use streaming::{
    EventQueueManager, EventQueueReader, EventQueueWriter, InMemoryQueueReader, InMemoryQueueWriter,
};
