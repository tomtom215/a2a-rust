// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Builder for [`RequestHandler`].
//!
//! [`RequestHandlerBuilder`] provides a fluent API for constructing a
//! [`RequestHandler`] with optional stores, push sender, interceptors,
//! and agent card.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::agent_card::AgentCard;

use crate::error::ServerResult;
use crate::executor::AgentExecutor;
use crate::handler::{HandlerLimits, RequestHandler};
use crate::interceptor::{ServerInterceptor, ServerInterceptorChain};
use crate::metrics::{Metrics, NoopMetrics};
use crate::push::{InMemoryPushConfigStore, PushConfigStore, PushSender};
use crate::store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};
use crate::streaming::EventQueueManager;
use crate::tenant_config::PerTenantConfig;
use crate::tenant_resolver::TenantResolver;

/// Fluent builder for [`RequestHandler`].
///
/// # Required
///
/// - `executor`: Any [`AgentExecutor`] implementation (passed as a concrete
///   type; the builder erases it to `Arc<dyn AgentExecutor>` during
///   [`build`](Self::build)).
///
/// # Optional (with defaults)
///
/// - `task_store`: defaults to [`InMemoryTaskStore`].
/// - `push_config_store`: defaults to [`InMemoryPushConfigStore`].
/// - `push_sender`: defaults to `None`.
/// - `interceptors`: defaults to an empty chain.
/// - `agent_card`: defaults to `None`.
/// - `tenant_resolver`: defaults to `None` (no tenant resolution).
/// - `tenant_config`: defaults to `None` (no per-tenant limits).
pub struct RequestHandlerBuilder {
    executor: Arc<dyn AgentExecutor>,
    task_store: Option<Arc<dyn TaskStore>>,
    task_store_config: TaskStoreConfig,
    push_config_store: Option<Arc<dyn PushConfigStore>>,
    push_sender: Option<Arc<dyn PushSender>>,
    interceptors: ServerInterceptorChain,
    agent_card: Option<AgentCard>,
    executor_timeout: Option<Duration>,
    event_queue_capacity: Option<usize>,
    max_event_size: Option<usize>,
    max_concurrent_streams: Option<usize>,
    event_queue_write_timeout: Option<Duration>,
    metrics: Arc<dyn Metrics>,
    handler_limits: HandlerLimits,
    tenant_resolver: Option<Arc<dyn TenantResolver>>,
    tenant_config: Option<PerTenantConfig>,
}

impl RequestHandlerBuilder {
    /// Creates a new builder with the given executor.
    ///
    /// The executor is type-erased to `Arc<dyn AgentExecutor>`.
    #[must_use]
    pub fn new(executor: impl AgentExecutor) -> Self {
        Self {
            executor: Arc::new(executor),
            task_store: None,
            task_store_config: TaskStoreConfig::default(),
            push_config_store: None,
            push_sender: None,
            interceptors: ServerInterceptorChain::new(),
            agent_card: None,
            executor_timeout: None,
            event_queue_capacity: None,
            max_event_size: None,
            max_concurrent_streams: None,
            event_queue_write_timeout: None,
            metrics: Arc::new(NoopMetrics),
            handler_limits: HandlerLimits::default(),
            tenant_resolver: None,
            tenant_config: None,
        }
    }

    /// Sets a custom task store.
    #[must_use]
    pub fn with_task_store(mut self, store: impl TaskStore + 'static) -> Self {
        self.task_store = Some(Arc::new(store));
        self
    }

    /// Sets a custom task store from an existing `Arc`.
    ///
    /// Use this when you want to share a store instance across multiple
    /// handlers or access it from background tasks.
    #[must_use]
    pub fn with_task_store_arc(mut self, store: Arc<dyn TaskStore>) -> Self {
        self.task_store = Some(store);
        self
    }

    /// Configures the default [`InMemoryTaskStore`] with custom TTL and capacity settings.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if a custom task store has already been set via
    /// [`with_task_store`](Self::with_task_store), since the config would be
    /// silently ignored.
    #[must_use]
    pub fn with_task_store_config(mut self, config: TaskStoreConfig) -> Self {
        debug_assert!(
            self.task_store.is_none(),
            "with_task_store_config() called after with_task_store(); \
             the config will be ignored because a custom store was already set"
        );
        self.task_store_config = config;
        self
    }

    /// Sets a custom push configuration store.
    #[must_use]
    pub fn with_push_config_store(mut self, store: impl PushConfigStore + 'static) -> Self {
        self.push_config_store = Some(Arc::new(store));
        self
    }

    /// Sets a push notification sender.
    #[must_use]
    pub fn with_push_sender(mut self, sender: impl PushSender + 'static) -> Self {
        self.push_sender = Some(Arc::new(sender));
        self
    }

    /// Adds a server interceptor to the chain.
    #[must_use]
    pub fn with_interceptor(mut self, interceptor: impl ServerInterceptor + 'static) -> Self {
        self.interceptors.push(Arc::new(interceptor));
        self
    }

    /// Sets a timeout for executor execution.
    ///
    /// If the executor does not complete within this duration, the task is
    /// marked as failed with a timeout error.
    #[must_use]
    pub const fn with_executor_timeout(mut self, timeout: Duration) -> Self {
        self.executor_timeout = Some(timeout);
        self
    }

    /// Sets the agent card for discovery responses.
    #[must_use]
    pub fn with_agent_card(mut self, card: AgentCard) -> Self {
        self.agent_card = Some(card);
        self
    }

    /// Sets the event queue channel capacity for streaming.
    ///
    /// Defaults to 64 items. Higher values allow more events to be buffered
    /// before backpressure is applied.
    #[must_use]
    pub const fn with_event_queue_capacity(mut self, capacity: usize) -> Self {
        self.event_queue_capacity = Some(capacity);
        self
    }

    /// Sets the maximum serialized event size in bytes.
    ///
    /// Events exceeding this size are rejected to prevent OOM conditions.
    /// Defaults to 16 MiB.
    #[must_use]
    pub const fn with_max_event_size(mut self, max_event_size: usize) -> Self {
        self.max_event_size = Some(max_event_size);
        self
    }

    /// Sets the maximum number of concurrent streaming event queues.
    ///
    /// Limits memory usage from concurrent streams. When the limit is reached,
    /// new streaming requests will fail.
    #[must_use]
    pub const fn with_max_concurrent_streams(mut self, max: usize) -> Self {
        self.max_concurrent_streams = Some(max);
        self
    }

    /// Sets the write timeout for event queue sends.
    ///
    /// Prevents executors from blocking indefinitely when a client is slow or
    /// disconnected. Default: 5 seconds.
    #[must_use]
    pub const fn with_event_queue_write_timeout(mut self, timeout: Duration) -> Self {
        self.event_queue_write_timeout = Some(timeout);
        self
    }

    /// Sets configurable limits for the handler (ID lengths, metadata size, etc.).
    ///
    /// Defaults to [`HandlerLimits::default()`].
    #[must_use]
    pub const fn with_handler_limits(mut self, limits: HandlerLimits) -> Self {
        self.handler_limits = limits;
        self
    }

    /// Sets a metrics observer for handler activity.
    ///
    /// Defaults to [`NoopMetrics`] which discards all events.
    #[must_use]
    pub fn with_metrics(mut self, metrics: impl Metrics + 'static) -> Self {
        self.metrics = Arc::new(metrics);
        self
    }

    /// Sets a tenant resolver for multi-tenant deployments.
    ///
    /// The resolver extracts a tenant identifier from each incoming request's
    /// [`CallContext`](crate::CallContext). When combined with
    /// [`with_tenant_config`](Self::with_tenant_config), this enables per-tenant
    /// resource limits and configuration.
    ///
    /// Defaults to `None` (single-tenant mode).
    #[must_use]
    pub fn with_tenant_resolver(mut self, resolver: impl TenantResolver) -> Self {
        self.tenant_resolver = Some(Arc::new(resolver));
        self
    }

    /// Sets per-tenant configuration for multi-tenant deployments.
    ///
    /// [`PerTenantConfig`] allows differentiated service levels (timeouts,
    /// capacity limits, rate limits) per tenant. Pair with
    /// [`with_tenant_resolver`](Self::with_tenant_resolver) to extract the
    /// tenant identity from incoming requests.
    ///
    /// Defaults to `None` (uniform limits for all callers).
    #[must_use]
    pub fn with_tenant_config(mut self, config: PerTenantConfig) -> Self {
        self.tenant_config = Some(config);
        self
    }

    /// Builds the [`RequestHandler`].
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::InvalidParams`](crate::error::ServerError::InvalidParams) if the configuration is invalid:
    /// - Agent card with empty `supported_interfaces`
    /// - Zero executor timeout (would cause immediate timeouts)
    #[allow(clippy::too_many_lines)]
    pub fn build(self) -> ServerResult<RequestHandler> {
        // Validate agent card if provided.
        if let Some(ref card) = self.agent_card {
            if card.supported_interfaces.is_empty() {
                return Err(crate::error::ServerError::InvalidParams(
                    "agent card must have at least one supported interface".into(),
                ));
            }
        }

        // Validate executor timeout is not zero.
        if let Some(timeout) = self.executor_timeout {
            if timeout.is_zero() {
                return Err(crate::error::ServerError::InvalidParams(
                    "executor timeout must be greater than zero".into(),
                ));
            }
        }

        // Validate handler limits are sensible (zero values cause all requests to fail).
        if self.handler_limits.max_id_length == 0 {
            return Err(crate::error::ServerError::InvalidParams(
                "max_id_length must be greater than zero".into(),
            ));
        }
        if self.handler_limits.max_metadata_size == 0 {
            return Err(crate::error::ServerError::InvalidParams(
                "max_metadata_size must be greater than zero".into(),
            ));
        }
        if self.handler_limits.push_delivery_timeout.is_zero() {
            return Err(crate::error::ServerError::InvalidParams(
                "push_delivery_timeout must be greater than zero".into(),
            ));
        }

        Ok(RequestHandler {
            executor: self.executor,
            task_store: self.task_store.unwrap_or_else(|| {
                Arc::new(InMemoryTaskStore::with_config(self.task_store_config))
            }),
            push_config_store: self
                .push_config_store
                .unwrap_or_else(|| Arc::new(InMemoryPushConfigStore::new())),
            push_sender: self.push_sender,
            event_queue_manager: {
                let mut mgr = self
                    .event_queue_capacity
                    .map_or_else(EventQueueManager::new, EventQueueManager::with_capacity);
                if let Some(max_size) = self.max_event_size {
                    mgr = mgr.with_max_event_size(max_size);
                }
                if let Some(timeout) = self.event_queue_write_timeout {
                    mgr = mgr.with_write_timeout(timeout);
                }
                if let Some(max_streams) = self.max_concurrent_streams {
                    mgr = mgr.with_max_concurrent_queues(max_streams);
                }
                mgr = mgr.with_metrics(Arc::clone(&self.metrics));
                mgr
            },
            interceptors: self.interceptors,
            agent_card: self.agent_card,
            executor_timeout: self.executor_timeout,
            metrics: self.metrics,
            limits: self.handler_limits,
            tenant_resolver: self.tenant_resolver,
            tenant_config: self.tenant_config,
            cancellation_tokens: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            context_locks: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        })
    }
}

impl std::fmt::Debug for RequestHandlerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestHandlerBuilder")
            .field("executor", &"<dyn AgentExecutor>")
            .field("task_store", &self.task_store.is_some())
            .field("task_store_config", &self.task_store_config)
            .field("push_config_store", &self.push_config_store.is_some())
            .field("push_sender", &self.push_sender.is_some())
            .field("interceptors", &self.interceptors)
            .field("agent_card", &self.agent_card.is_some())
            .field("executor_timeout", &self.executor_timeout)
            .field("event_queue_capacity", &self.event_queue_capacity)
            .field("max_event_size", &self.max_event_size)
            .field("max_concurrent_streams", &self.max_concurrent_streams)
            .field("event_queue_write_timeout", &self.event_queue_write_timeout)
            .field("metrics", &"<dyn Metrics>")
            .field("handler_limits", &self.handler_limits)
            .field("tenant_resolver", &self.tenant_resolver.is_some())
            .field("tenant_config", &self.tenant_config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_executor;

    struct TestExecutor;

    agent_executor!(TestExecutor, |_ctx, _queue| async { Ok(()) });

    #[test]
    fn builder_defaults_build_ok() {
        let handler = RequestHandlerBuilder::new(TestExecutor).build();
        assert!(handler.is_ok());
    }

    #[test]
    fn builder_zero_executor_timeout_errors() {
        let result = RequestHandlerBuilder::new(TestExecutor)
            .with_executor_timeout(Duration::ZERO)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_empty_agent_card_interfaces_errors() {
        use a2a_protocol_types::{AgentCapabilities, AgentCard};

        let card = AgentCard {
            url: None,
            name: "empty".into(),
            version: "1.0".into(),
            description: "No interfaces".into(),
            supported_interfaces: vec![],
            provider: None,
            icon_url: None,
            documentation_url: None,
            capabilities: AgentCapabilities::none(),
            security_schemes: None,
            security_requirements: None,
            default_input_modes: vec![],
            default_output_modes: vec![],
            skills: vec![],
            signatures: None,
        };

        let result = RequestHandlerBuilder::new(TestExecutor)
            .with_agent_card(card)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_with_all_options() {
        use a2a_protocol_types::{AgentCapabilities, AgentCard, AgentInterface};

        let card = AgentCard {
            url: None,
            name: "test".into(),
            version: "1.0".into(),
            description: "Test agent".into(),
            supported_interfaces: vec![AgentInterface {
                url: "http://localhost:8080".into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            }],
            provider: None,
            icon_url: None,
            documentation_url: None,
            capabilities: AgentCapabilities::none(),
            security_schemes: None,
            security_requirements: None,
            default_input_modes: vec![],
            default_output_modes: vec![],
            skills: vec![],
            signatures: None,
        };

        let result = RequestHandlerBuilder::new(TestExecutor)
            .with_agent_card(card)
            .with_executor_timeout(Duration::from_secs(30))
            .with_event_queue_capacity(128)
            .with_max_event_size(1024 * 1024)
            .with_max_concurrent_streams(10)
            .with_handler_limits(HandlerLimits::default().with_max_id_length(2048))
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn builder_with_tenant_resolver_and_config() {
        use crate::tenant_config::{PerTenantConfig, TenantLimits};
        use crate::tenant_resolver::HeaderTenantResolver;

        let handler = RequestHandlerBuilder::new(TestExecutor)
            .with_tenant_resolver(HeaderTenantResolver::default())
            .with_tenant_config(
                PerTenantConfig::builder()
                    .default_limits(TenantLimits::builder().rate_limit_rps(100).build())
                    .with_override(
                        "premium",
                        TenantLimits::builder().rate_limit_rps(1000).build(),
                    )
                    .build(),
            )
            .build();
        assert!(handler.is_ok());

        let handler = handler.unwrap();
        assert!(handler.tenant_resolver().is_some());
        assert!(handler.tenant_config().is_some());
        assert_eq!(
            handler
                .tenant_config()
                .unwrap()
                .get("premium")
                .rate_limit_rps,
            Some(1000)
        );
        assert_eq!(
            handler
                .tenant_config()
                .unwrap()
                .get("unknown")
                .rate_limit_rps,
            Some(100)
        );
    }

    #[test]
    fn builder_without_tenant_fields() {
        let handler = RequestHandlerBuilder::new(TestExecutor).build().unwrap();
        assert!(handler.tenant_resolver().is_none());
        assert!(handler.tenant_config().is_none());
    }

    #[test]
    fn builder_debug_does_not_panic() {
        let builder = RequestHandlerBuilder::new(TestExecutor);
        let debug = format!("{builder:?}");
        assert!(debug.contains("RequestHandlerBuilder"));
    }

    #[test]
    fn builder_with_push_config_store_builds_ok() {
        use crate::push::InMemoryPushConfigStore;
        let result = RequestHandlerBuilder::new(TestExecutor)
            .with_push_config_store(InMemoryPushConfigStore::new())
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn builder_with_event_queue_write_timeout_builds_ok() {
        let result = RequestHandlerBuilder::new(TestExecutor)
            .with_event_queue_write_timeout(Duration::from_secs(10))
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn builder_zero_max_id_length_errors() {
        let result = RequestHandlerBuilder::new(TestExecutor)
            .with_handler_limits(HandlerLimits::default().with_max_id_length(0))
            .build();
        assert!(result.is_err(), "zero max_id_length should be rejected");
    }

    #[test]
    fn builder_zero_max_metadata_size_errors() {
        let result = RequestHandlerBuilder::new(TestExecutor)
            .with_handler_limits(HandlerLimits::default().with_max_metadata_size(0))
            .build();
        assert!(result.is_err(), "zero max_metadata_size should be rejected");
    }

    #[test]
    fn builder_zero_push_delivery_timeout_errors() {
        let result = RequestHandlerBuilder::new(TestExecutor)
            .with_handler_limits(
                HandlerLimits::default().with_push_delivery_timeout(Duration::ZERO),
            )
            .build();
        assert!(
            result.is_err(),
            "zero push_delivery_timeout should be rejected"
        );
    }
}
