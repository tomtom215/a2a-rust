// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Builder for [`RequestHandler`].
//!
//! [`RequestHandlerBuilder`] provides a fluent API for constructing a
//! [`RequestHandler`] with optional stores, push sender, interceptors,
//! and agent card.

use std::sync::Arc;
use std::time::Duration;

use a2a_types::agent_card::AgentCard;

use crate::error::ServerResult;
use crate::executor::AgentExecutor;
use crate::handler::RequestHandler;
use crate::interceptor::{ServerInterceptor, ServerInterceptorChain};
use crate::metrics::{Metrics, NoopMetrics};
use crate::push::{InMemoryPushConfigStore, PushConfigStore, PushSender};
use crate::store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};
use crate::streaming::EventQueueManager;

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
pub struct RequestHandlerBuilder {
    executor: Arc<dyn AgentExecutor>,
    task_store: Option<Box<dyn TaskStore>>,
    task_store_config: TaskStoreConfig,
    push_config_store: Option<Box<dyn PushConfigStore>>,
    push_sender: Option<Box<dyn PushSender>>,
    interceptors: ServerInterceptorChain,
    agent_card: Option<AgentCard>,
    executor_timeout: Option<Duration>,
    event_queue_capacity: Option<usize>,
    max_event_size: Option<usize>,
    max_concurrent_streams: Option<usize>,
    metrics: Box<dyn Metrics>,
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
            metrics: Box::new(NoopMetrics),
        }
    }

    /// Sets a custom task store.
    #[must_use]
    pub fn with_task_store(mut self, store: impl TaskStore + 'static) -> Self {
        self.task_store = Some(Box::new(store));
        self
    }

    /// Configures the default [`InMemoryTaskStore`] with custom TTL and capacity settings.
    ///
    /// This is ignored if a custom task store is set via [`with_task_store`](Self::with_task_store).
    #[must_use]
    pub const fn with_task_store_config(mut self, config: TaskStoreConfig) -> Self {
        self.task_store_config = config;
        self
    }

    /// Sets a custom push configuration store.
    #[must_use]
    pub fn with_push_config_store(mut self, store: impl PushConfigStore + 'static) -> Self {
        self.push_config_store = Some(Box::new(store));
        self
    }

    /// Sets a push notification sender.
    #[must_use]
    pub fn with_push_sender(mut self, sender: impl PushSender + 'static) -> Self {
        self.push_sender = Some(Box::new(sender));
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

    /// Sets a metrics observer for handler activity.
    ///
    /// Defaults to [`NoopMetrics`] which discards all events.
    #[must_use]
    pub fn with_metrics(mut self, metrics: impl Metrics + 'static) -> Self {
        self.metrics = Box::new(metrics);
        self
    }

    /// Builds the [`RequestHandler`].
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::InvalidParams`](crate::error::ServerError::InvalidParams) if the configuration is invalid:
    /// - Agent card with empty `supported_interfaces`
    /// - Zero executor timeout (would cause immediate timeouts)
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

        Ok(RequestHandler {
            executor: self.executor,
            task_store: self.task_store.unwrap_or_else(|| {
                Box::new(InMemoryTaskStore::with_config(self.task_store_config))
            }),
            push_config_store: self
                .push_config_store
                .unwrap_or_else(|| Box::new(InMemoryPushConfigStore::new())),
            push_sender: self.push_sender,
            event_queue_manager: {
                let mut mgr = self
                    .event_queue_capacity
                    .map_or_else(EventQueueManager::new, EventQueueManager::with_capacity);
                if let Some(max_size) = self.max_event_size {
                    mgr = mgr.with_max_event_size(max_size);
                }
                if let Some(max_streams) = self.max_concurrent_streams {
                    mgr = mgr.with_max_concurrent_queues(max_streams);
                }
                mgr
            },
            interceptors: self.interceptors,
            agent_card: self.agent_card,
            executor_timeout: self.executor_timeout,
            metrics: self.metrics,
            cancellation_tokens: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
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
            .field("metrics", &"<dyn Metrics>")
            .finish()
    }
}
