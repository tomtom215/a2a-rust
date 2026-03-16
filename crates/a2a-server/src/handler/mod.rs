// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Core request handler — protocol logic layer.
//!
//! [`RequestHandler`] wires together the executor, stores, push sender,
//! interceptors, and event queue manager to implement all A2A v1.0 methods.
//!
//! # Module overview
//!
//! | Module | Contents |
//! |---|---|
//! | [`limits`] | [`HandlerLimits`] — configurable per-handler bounds |
//! | [`messaging`] | [`RequestHandler::on_send_message`] — send/stream entry point |
//! | [`lifecycle`] | Get, list, cancel, resubscribe, extended agent card |
//! | [`push_config`] | Push notification config CRUD |
//! | [`event_processing`] | Event collection, state transitions, push delivery |
//! | [`shutdown`] | Graceful shutdown with optional timeout |

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
    /// Cancellation tokens for in-flight tasks (keyed by [`TaskId`]).
    pub(crate) cancellation_tokens: Arc<tokio::sync::RwLock<HashMap<TaskId, CancellationEntry>>>,
}

/// Entry in the cancellation token map, tracking creation time for eviction.
#[derive(Debug, Clone)]
pub(crate) struct CancellationEntry {
    /// The cancellation token.
    pub(crate) token: tokio_util::sync::CancellationToken,
    /// When this entry was created (for time-based eviction).
    pub(crate) created_at: Instant,
}

impl std::fmt::Debug for RequestHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestHandler")
            .field("push_sender", &self.push_sender.is_some())
            .field("event_queue_manager", &self.event_queue_manager)
            .field("interceptors", &self.interceptors)
            .field("agent_card", &self.agent_card.is_some())
            .field("metrics", &"<dyn Metrics>")
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
