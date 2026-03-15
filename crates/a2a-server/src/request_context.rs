// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Request context passed to the [`AgentExecutor`](crate::AgentExecutor).
//!
//! [`RequestContext`] bundles together the incoming message, task identifiers,
//! and any previously stored task snapshot so that the executor has all the
//! information it needs to process a request.

use a2a_types::message::Message;
use a2a_types::task::{Task, TaskId};

/// Context for a single agent execution request.
///
/// Built by the [`RequestHandler`](crate::RequestHandler) and passed to
/// [`AgentExecutor::execute`](crate::AgentExecutor::execute).
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// The incoming user message.
    pub message: Message,

    /// The task identifier for this execution.
    pub task_id: TaskId,

    /// The conversation context identifier.
    pub context_id: String,

    /// The previously stored task snapshot, if this is a continuation.
    pub stored_task: Option<Task>,

    /// Arbitrary metadata from the request.
    pub metadata: Option<serde_json::Value>,
}

impl RequestContext {
    /// Creates a new [`RequestContext`].
    #[must_use]
    pub const fn new(message: Message, task_id: TaskId, context_id: String) -> Self {
        Self {
            message,
            task_id,
            context_id,
            stored_task: None,
            metadata: None,
        }
    }

    /// Sets the stored task snapshot for continuation requests.
    #[must_use]
    pub fn with_stored_task(mut self, task: Task) -> Self {
        self.stored_task = Some(task);
        self
    }

    /// Sets request metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}
