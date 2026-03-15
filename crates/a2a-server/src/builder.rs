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
use crate::push::{InMemoryPushConfigStore, PushConfigStore, PushSender};
use crate::store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};
use crate::streaming::EventQueueManager;

/// Fluent builder for [`RequestHandler`].
///
/// # Required
///
/// - `executor`: The [`AgentExecutor`] implementation.
///
/// # Optional (with defaults)
///
/// - `task_store`: defaults to [`InMemoryTaskStore`].
/// - `push_config_store`: defaults to [`InMemoryPushConfigStore`].
/// - `push_sender`: defaults to `None`.
/// - `interceptors`: defaults to an empty chain.
/// - `agent_card`: defaults to `None`.
pub struct RequestHandlerBuilder<E: AgentExecutor> {
    executor: E,
    task_store: Option<Box<dyn TaskStore>>,
    task_store_config: TaskStoreConfig,
    push_config_store: Option<Box<dyn PushConfigStore>>,
    push_sender: Option<Box<dyn PushSender>>,
    interceptors: ServerInterceptorChain,
    agent_card: Option<AgentCard>,
    executor_timeout: Option<Duration>,
}

impl<E: AgentExecutor> RequestHandlerBuilder<E> {
    /// Creates a new builder with the given executor.
    #[must_use]
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            task_store: None,
            task_store_config: TaskStoreConfig::default(),
            push_config_store: None,
            push_sender: None,
            interceptors: ServerInterceptorChain::new(),
            agent_card: None,
            executor_timeout: None,
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

    /// Builds the [`RequestHandler`].
    ///
    /// # Errors
    ///
    /// Currently infallible but returns [`ServerResult`] for future extensibility.
    pub fn build(self) -> ServerResult<RequestHandler<E>> {
        Ok(RequestHandler {
            executor: Arc::new(self.executor),
            task_store: self.task_store.unwrap_or_else(|| {
                Box::new(InMemoryTaskStore::with_config(self.task_store_config))
            }),
            push_config_store: self
                .push_config_store
                .unwrap_or_else(|| Box::new(InMemoryPushConfigStore::new())),
            push_sender: self.push_sender,
            event_queue_manager: EventQueueManager::new(),
            interceptors: self.interceptors,
            agent_card: self.agent_card,
            executor_timeout: self.executor_timeout,
            cancellation_tokens: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        })
    }
}

impl<E: AgentExecutor> std::fmt::Debug for RequestHandlerBuilder<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestHandlerBuilder")
            .field("executor", &"...")
            .field("task_store", &self.task_store.is_some())
            .field("task_store_config", &self.task_store_config)
            .field("push_config_store", &self.push_config_store.is_some())
            .field("push_sender", &self.push_sender.is_some())
            .field("interceptors", &self.interceptors)
            .field("agent_card", &self.agent_card.is_some())
            .field("executor_timeout", &self.executor_timeout)
            .finish()
    }
}
