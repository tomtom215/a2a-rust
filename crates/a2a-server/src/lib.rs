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
//! | [`executor_helpers`] | [`boxed_future`], [`agent_executor!`] macro |
//! | [`handler`] | [`RequestHandler`], [`SendMessageResult`], [`HandlerLimits`] |
//! | [`builder`] | [`RequestHandlerBuilder`] |
//! | [`store`] | [`TaskStore`], [`InMemoryTaskStore`], `SqliteTaskStore` (sqlite feature) |
//! | [`streaming`] | Event queues, SSE response builder |
//! | [`push`] | Push config store, push sender |
//! | [`agent_card`] | Static/dynamic agent card handlers |
//! | [`serve`](mod@serve) | [`serve()`](serve::serve), [`serve_with_addr`], [`Dispatcher`] |
//! | [`dispatch`] | [`JsonRpcDispatcher`], [`RestDispatcher`] |
//! | [`interceptor`] | [`ServerInterceptor`], [`ServerInterceptorChain`] |
//! | [`rate_limit`] | [`RateLimitInterceptor`], [`RateLimitConfig`] |
//! | [`request_context`] | [`RequestContext`] |
//! | [`call_context`] | [`CallContext`] (includes HTTP headers for auth) |
//! | [`metrics`] | [`Metrics`] trait (request counts, latency, errors) |
//! | [`tenant_resolver`] | [`TenantResolver`], [`HeaderTenantResolver`], [`BearerTokenTenantResolver`], [`PathSegmentTenantResolver`] |
//! | [`tenant_config`] | [`PerTenantConfig`], [`TenantLimits`] |
//!
//! # gRPC transport
//!
//! Enable the `grpc` feature flag to use `GrpcDispatcher` for gRPC
//! transport (tonic-backed). See the `dispatch::grpc` module for details.
//!
//! # Rate limiting
//!
//! Built-in rate limiting is available via [`RateLimitInterceptor`],
//! a fixed-window per-caller interceptor. For advanced use cases (sliding windows,
//! distributed counters), use a reverse proxy (nginx, Envoy) or a custom
//! [`ServerInterceptor`].

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
pub mod executor_helpers;
pub mod handler;
pub mod interceptor;
pub mod metrics;
pub mod push;
pub mod rate_limit;
pub mod request_context;
pub mod serve;
pub mod store;
pub mod streaming;
pub mod tenant_config;
pub mod tenant_resolver;

#[cfg(feature = "otel")]
pub mod otel;

// ── Flat re-exports ───────────────────────────────────────────────────────────

pub use agent_card::{
    AgentCardProducer, DynamicAgentCardHandler, HotReloadAgentCardHandler,
    StaticAgentCardHandler, CORS_ALLOW_ALL,
};
pub use builder::RequestHandlerBuilder;
pub use call_context::CallContext;
#[cfg(feature = "websocket")]
pub use dispatch::WebSocketDispatcher;
pub use dispatch::{CorsConfig, DispatchConfig, JsonRpcDispatcher, RestDispatcher};
#[cfg(feature = "grpc")]
pub use dispatch::{GrpcConfig, GrpcDispatcher};
pub use error::{ServerError, ServerResult};
pub use executor::AgentExecutor;
pub use executor_helpers::{boxed_future, EventEmitter};
pub use handler::{HandlerLimits, RequestHandler, SendMessageResult};
pub use interceptor::{ServerInterceptor, ServerInterceptorChain};
pub use metrics::{ConnectionPoolStats, Metrics};
#[cfg(feature = "otel")]
pub use otel::OtelMetrics;
pub use push::{
    HttpPushSender, InMemoryPushConfigStore, PushConfigStore, PushRetryPolicy, PushSender,
    TenantAwareInMemoryPushConfigStore,
};
pub use rate_limit::{RateLimitConfig, RateLimitInterceptor};
pub use request_context::RequestContext;
pub use serve::{serve, serve_with_addr, Dispatcher};
pub use store::{
    InMemoryTaskStore, TaskStore, TaskStoreConfig, TenantAwareInMemoryTaskStore, TenantContext,
    TenantStoreConfig,
};

#[cfg(feature = "sqlite")]
pub use push::{SqlitePushConfigStore, TenantAwareSqlitePushConfigStore};
#[cfg(feature = "sqlite")]
pub use store::{Migration, MigrationRunner, SqliteTaskStore, TenantAwareSqliteTaskStore};
pub use streaming::{
    EventQueueManager, EventQueueReader, EventQueueWriter, InMemoryQueueReader, InMemoryQueueWriter,
};
pub use tenant_config::{PerTenantConfig, TenantLimits};
pub use tenant_resolver::{
    BearerTokenTenantResolver, HeaderTenantResolver, PathSegmentTenantResolver, TenantResolver,
};
