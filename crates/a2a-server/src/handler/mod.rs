// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Core request handler — protocol logic layer.
//!
//! [`RequestHandler`] wires together the executor, stores, push sender,
//! interceptors, and event queue manager to implement all A2A v1.0 methods.
//!
//! # Module overview
//!
//! | Module | Contents |
//! |---|---|
//! | `limits` | [`HandlerLimits`] — configurable per-handler bounds |
//! | `messaging` | `RequestHandler::on_send_message` — send/stream entry point |
//! | `lifecycle` | Get, list, cancel, resubscribe, extended agent card |
//! | `push_config` | Push notification config CRUD |
//! | `event_processing` | Event collection, state transitions, push delivery |
//! | `shutdown` | Graceful shutdown with optional timeout |

mod event_processing;
mod helpers;
mod lifecycle;
mod limits;
mod messaging;
mod push_config;
mod shutdown;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::task::TaskId;

use crate::executor::AgentExecutor;
use crate::interceptor::ServerInterceptorChain;
use crate::metrics::Metrics;
use crate::push::{PushConfigStore, PushSender};
use crate::store::TaskStore;
use crate::streaming::{EventQueueManager, InMemoryQueueReader};
use crate::tenant_config::PerTenantConfig;
use crate::tenant_resolver::TenantResolver;

pub use limits::HandlerLimits;

// Re-export the response type alongside the handler.
pub use a2a_protocol_types::responses::SendMessageResponse;

/// The core protocol logic handler.
///
/// Orchestrates task lifecycle, event streaming, push notifications, and
/// interceptor chains for all A2A methods.
///
/// `RequestHandler` is **not** generic — it stores the executor as
/// `Arc<dyn AgentExecutor>`, enabling dynamic dispatch and simplifying
/// the downstream API (dispatchers, builder, etc.).
///
/// # Store ownership
///
/// Stores are held as `Arc<dyn TaskStore>` / `Arc<dyn PushConfigStore>`
/// rather than `Box<dyn ...>` so that they can be cheaply cloned into
/// background tasks (e.g. the streaming push-delivery processor).
pub struct RequestHandler {
    pub(crate) executor: Arc<dyn AgentExecutor>,
    pub(crate) task_store: Arc<dyn TaskStore>,
    pub(crate) push_config_store: Arc<dyn PushConfigStore>,
    pub(crate) push_sender: Option<Arc<dyn PushSender>>,
    pub(crate) event_queue_manager: EventQueueManager,
    pub(crate) interceptors: ServerInterceptorChain,
    pub(crate) agent_card: Option<AgentCard>,
    pub(crate) executor_timeout: Option<Duration>,
    pub(crate) metrics: Arc<dyn Metrics>,
    pub(crate) limits: HandlerLimits,
    pub(crate) tenant_resolver: Option<Arc<dyn TenantResolver>>,
    pub(crate) tenant_config: Option<PerTenantConfig>,
    /// Cancellation tokens for in-flight tasks (keyed by [`TaskId`]).
    pub(crate) cancellation_tokens: Arc<tokio::sync::RwLock<HashMap<TaskId, CancellationEntry>>>,
    /// Per-context-ID locks to serialize find + save operations for the same
    /// context, preventing two concurrent `SendMessage` requests from both
    /// creating new tasks for the same `context_id`.
    pub(crate) context_locks:
        Arc<tokio::sync::RwLock<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

/// Entry in the cancellation token map, tracking creation time for eviction.
#[derive(Debug, Clone)]
pub(crate) struct CancellationEntry {
    /// The cancellation token.
    pub(crate) token: tokio_util::sync::CancellationToken,
    /// When this entry was created (for time-based eviction).
    pub(crate) created_at: Instant,
}

impl RequestHandler {
    /// Returns the tenant resolver, if configured.
    ///
    /// Use this in dispatchers or middleware to resolve the tenant identity
    /// from a [`CallContext`](crate::CallContext) before processing a request.
    #[must_use]
    pub fn tenant_resolver(&self) -> Option<&dyn TenantResolver> {
        self.tenant_resolver.as_deref()
    }

    /// Returns the per-tenant configuration, if configured.
    ///
    /// Use this alongside [`tenant_resolver`](Self::tenant_resolver) to look up
    /// resource limits for the resolved tenant.
    #[must_use]
    pub const fn tenant_config(&self) -> Option<&PerTenantConfig> {
        self.tenant_config.as_ref()
    }
}

impl std::fmt::Debug for RequestHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestHandler")
            .field("push_sender", &self.push_sender.is_some())
            .field("event_queue_manager", &self.event_queue_manager)
            .field("interceptors", &self.interceptors)
            .field("agent_card", &self.agent_card.is_some())
            .field("metrics", &"<dyn Metrics>")
            .field("tenant_resolver", &self.tenant_resolver.is_some())
            .field("tenant_config", &self.tenant_config)
            .finish_non_exhaustive()
    }
}

/// Result of [`RequestHandler::on_send_message`].
#[allow(clippy::large_enum_variant)]
pub enum SendMessageResult {
    /// A synchronous JSON-RPC response.
    Response(SendMessageResponse),
    /// A streaming SSE reader.
    Stream(InMemoryQueueReader),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_executor;
    use crate::builder::RequestHandlerBuilder;
    use crate::tenant_config::{PerTenantConfig, TenantLimits};
    use crate::tenant_resolver::HeaderTenantResolver;

    struct DummyExecutor;
    agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

    // ── Construction with defaults ───────────────────────────────────────

    #[test]
    fn default_build_has_no_tenant_resolver() {
        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .build()
            .expect("default build should succeed");
        assert!(
            handler.tenant_resolver().is_none(),
            "default handler should have no tenant resolver"
        );
    }

    #[test]
    fn default_build_has_no_tenant_config() {
        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .build()
            .expect("default build should succeed");
        assert!(
            handler.tenant_config().is_none(),
            "default handler should have no tenant config"
        );
    }

    // ── tenant_resolver() accessor ───────────────────────────────────────

    #[test]
    fn tenant_resolver_returns_some_when_configured() {
        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_tenant_resolver(HeaderTenantResolver::default())
            .build()
            .expect("build with tenant resolver");
        assert!(
            handler.tenant_resolver().is_some(),
            "should return Some when a resolver was configured"
        );
    }

    #[test]
    fn tenant_resolver_returns_none_when_not_configured() {
        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .build()
            .expect("default build");
        assert!(
            handler.tenant_resolver().is_none(),
            "should return None when no resolver was configured"
        );
    }

    // ── tenant_config() accessor ─────────────────────────────────────────

    #[test]
    fn tenant_config_returns_some_when_configured() {
        let config = PerTenantConfig::builder()
            .default_limits(TenantLimits::builder().rate_limit_rps(50).build())
            .build();

        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_tenant_config(config)
            .build()
            .expect("build with tenant config");
        assert!(
            handler.tenant_config().is_some(),
            "should return Some when tenant config was provided"
        );
    }

    #[test]
    fn tenant_config_returns_none_when_not_configured() {
        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .build()
            .expect("default build");
        assert!(
            handler.tenant_config().is_none(),
            "should return None when no tenant config was provided"
        );
    }

    #[test]
    fn tenant_config_preserves_values() {
        let config = PerTenantConfig::builder()
            .default_limits(TenantLimits::builder().rate_limit_rps(100).build())
            .with_override("vip", TenantLimits::builder().rate_limit_rps(500).build())
            .build();

        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_tenant_config(config)
            .build()
            .expect("build with per-tenant overrides");

        let cfg = handler.tenant_config().expect("config should be Some");
        assert_eq!(cfg.get("vip").rate_limit_rps, Some(500));
        assert_eq!(cfg.get("unknown-tenant").rate_limit_rps, Some(100));
    }

    // ── Both tenant fields together ──────────────────────────────────────

    #[test]
    fn handler_with_both_tenant_fields() {
        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_tenant_resolver(HeaderTenantResolver::default())
            .with_tenant_config(
                PerTenantConfig::builder()
                    .default_limits(TenantLimits::builder().rate_limit_rps(10).build())
                    .build(),
            )
            .build()
            .expect("build with both tenant resolver and config");

        assert!(handler.tenant_resolver().is_some());
        assert!(handler.tenant_config().is_some());
    }

    // ── Debug impl ───────────────────────────────────────────────────────

    #[test]
    fn debug_impl_does_not_panic() {
        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .build()
            .expect("default build");
        let debug = format!("{handler:?}");
        assert!(
            debug.contains("RequestHandler"),
            "Debug output should contain struct name"
        );
    }

    #[test]
    fn debug_shows_tenant_resolver_presence() {
        let without = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
        let with = RequestHandlerBuilder::new(DummyExecutor)
            .with_tenant_resolver(HeaderTenantResolver::default())
            .build()
            .unwrap();

        let dbg_without = format!("{without:?}");
        let dbg_with = format!("{with:?}");

        assert!(
            dbg_without.contains("tenant_resolver: false"),
            "should show false when no resolver: {dbg_without}"
        );
        assert!(
            dbg_with.contains("tenant_resolver: true"),
            "should show true when resolver configured: {dbg_with}"
        );
    }

    // ── SendMessageResult variant construction ───────────────────────────

    #[test]
    fn send_message_result_response_variant() {
        use a2a_protocol_types::responses::SendMessageResponse;
        use a2a_protocol_types::task::{Task, TaskState, TaskStatus};

        let task = Task {
            id: "t1".into(),
            context_id: "c1".into(),
            status: TaskStatus::new(TaskState::Completed),
            artifacts: None,
            history: None,
            metadata: None,
        };
        let result = SendMessageResult::Response(SendMessageResponse::Task(task));
        assert!(matches!(result, SendMessageResult::Response(_)));
    }
}
