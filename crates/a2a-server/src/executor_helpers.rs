// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Ergonomic helpers for implementing [`AgentExecutor`](crate::AgentExecutor).
//!
//! The [`AgentExecutor`](crate::AgentExecutor) trait requires `Pin<Box<dyn Future>>`
//! return types for object safety. These helpers reduce the boilerplate.
//!
//! # `boxed_future` helper
//!
//! Wraps an `async` block into the `Pin<Box<dyn Future>>` form:
//!
//! ```rust
//! use a2a_protocol_server::executor_helpers::boxed_future;
//! use a2a_protocol_server::executor::AgentExecutor;
//! use a2a_protocol_server::request_context::RequestContext;
//! use a2a_protocol_server::streaming::EventQueueWriter;
//! use a2a_protocol_types::error::A2aResult;
//! use std::pin::Pin;
//! use std::future::Future;
//!
//! struct MyAgent;
//!
//! impl AgentExecutor for MyAgent {
//!     fn execute<'a>(
//!         &'a self,
//!         ctx: &'a RequestContext,
//!         queue: &'a dyn EventQueueWriter,
//!     ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
//!         boxed_future(async move {
//!             // Your logic here — no Box::pin wrapper needed!
//!             Ok(())
//!         })
//!     }
//! }
//! ```
//!
//! # `agent_executor!` macro
//!
//! Generates the full [`AgentExecutor`](crate::AgentExecutor) impl from plain
//! `async` bodies:
//!
//! ```rust
//! use a2a_protocol_server::agent_executor;
//! use a2a_protocol_server::request_context::RequestContext;
//! use a2a_protocol_server::streaming::EventQueueWriter;
//! use a2a_protocol_types::error::A2aResult;
//!
//! struct EchoAgent;
//!
//! agent_executor!(EchoAgent, |_ctx, _queue| async {
//!     Ok(())
//! });
//! ```

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use crate::request_context::RequestContext;
use crate::streaming::EventQueueWriter;

/// Wraps an async expression into `Pin<Box<dyn Future<Output = T> + Send + 'a>>`.
///
/// This is the minimal helper for reducing [`AgentExecutor`](crate::AgentExecutor)
/// boilerplate. Instead of:
///
/// ```rust,ignore
/// fn execute<'a>(...) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
///     Box::pin(async move { ... })
/// }
/// ```
///
/// You can write:
///
/// ```rust,ignore
/// fn execute<'a>(...) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
///     boxed_future(async move { ... })
/// }
/// ```
pub fn boxed_future<'a, T>(
    fut: impl Future<Output = T> + Send + 'a,
) -> Pin<Box<dyn Future<Output = T> + Send + 'a>> {
    Box::pin(fut)
}

/// Generates an [`AgentExecutor`](crate::AgentExecutor) implementation from a
/// closure-like syntax.
///
/// # Basic usage (execute only)
///
/// ```rust
/// use a2a_protocol_server::agent_executor;
///
/// struct MyAgent;
///
/// agent_executor!(MyAgent, |ctx, queue| async {
///     // ctx: &RequestContext, queue: &dyn EventQueueWriter
///     Ok(())
/// });
/// ```
///
/// # With cancel handler
///
/// ```rust
/// use a2a_protocol_server::agent_executor;
///
/// struct CancelableAgent;
///
/// agent_executor!(CancelableAgent,
///     execute: |ctx, queue| async { Ok(()) },
///     cancel: |ctx, queue| async { Ok(()) }
/// );
/// ```
#[macro_export]
macro_rules! agent_executor {
    // Simple form: just execute
    ($ty:ty, |$ctx:ident, $queue:ident| async $body:block) => {
        impl $crate::executor::AgentExecutor for $ty {
            fn execute<'a>(
                &'a self,
                $ctx: &'a $crate::request_context::RequestContext,
                $queue: &'a dyn $crate::streaming::EventQueueWriter,
            ) -> ::std::pin::Pin<
                ::std::boxed::Box<
                    dyn ::std::future::Future<
                            Output = ::a2a_protocol_types::error::A2aResult<()>,
                        > + ::std::marker::Send
                        + 'a,
                >,
            > {
                ::std::boxed::Box::pin(async move $body)
            }
        }
    };

    // Full form: execute + cancel
    ($ty:ty,
        execute: |$ctx:ident, $queue:ident| async $exec_body:block,
        cancel: |$cctx:ident, $cqueue:ident| async $cancel_body:block
    ) => {
        impl $crate::executor::AgentExecutor for $ty {
            fn execute<'a>(
                &'a self,
                $ctx: &'a $crate::request_context::RequestContext,
                $queue: &'a dyn $crate::streaming::EventQueueWriter,
            ) -> ::std::pin::Pin<
                ::std::boxed::Box<
                    dyn ::std::future::Future<
                            Output = ::a2a_protocol_types::error::A2aResult<()>,
                        > + ::std::marker::Send
                        + 'a,
                >,
            > {
                ::std::boxed::Box::pin(async move $exec_body)
            }

            fn cancel<'a>(
                &'a self,
                $cctx: &'a $crate::request_context::RequestContext,
                $cqueue: &'a dyn $crate::streaming::EventQueueWriter,
            ) -> ::std::pin::Pin<
                ::std::boxed::Box<
                    dyn ::std::future::Future<
                            Output = ::a2a_protocol_types::error::A2aResult<()>,
                        > + ::std::marker::Send
                        + 'a,
                >,
            > {
                ::std::boxed::Box::pin(async move $cancel_body)
            }
        }
    };
}

// ── EventEmitter ─────────────────────────────────────────────────────────────

/// Ergonomic helper for emitting status and artifact events from an executor.
///
/// Caches `task_id` and `context_id` from the [`RequestContext`] so that every
/// event emission is a one-liner instead of a 7-line struct literal.
///
/// # Example
///
/// ```rust,ignore
/// use a2a_protocol_server::executor_helpers::EventEmitter;
/// use a2a_protocol_types::task::TaskState;
/// use a2a_protocol_types::message::Part;
///
/// let emit = EventEmitter::new(ctx, queue);
/// emit.status(TaskState::Working).await?;
/// emit.artifact("result", vec![Part::text("hello")], None, Some(true)).await?;
/// emit.status(TaskState::Completed).await?;
/// ```
pub struct EventEmitter<'a> {
    /// The request context for this execution.
    pub ctx: &'a RequestContext,
    /// The event queue writer for this execution.
    pub queue: &'a dyn EventQueueWriter,
}

impl<'a> EventEmitter<'a> {
    /// Creates a new [`EventEmitter`] from the given context and queue.
    #[must_use]
    pub fn new(ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Self {
        Self { ctx, queue }
    }

    /// Emits a status update event.
    ///
    /// # Errors
    ///
    /// Returns an error if the event queue write fails.
    pub async fn status(&self, state: TaskState) -> A2aResult<()> {
        self.queue
            .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: self.ctx.task_id.clone(),
                context_id: ContextId::new(self.ctx.context_id.clone()),
                status: TaskStatus::new(state),
                metadata: None,
            }))
            .await
    }

    /// Emits an artifact update event.
    ///
    /// # Errors
    ///
    /// Returns an error if the event queue write fails.
    pub async fn artifact(
        &self,
        id: &str,
        parts: Vec<Part>,
        append: Option<bool>,
        last_chunk: Option<bool>,
    ) -> A2aResult<()> {
        self.queue
            .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                task_id: self.ctx.task_id.clone(),
                context_id: ContextId::new(self.ctx.context_id.clone()),
                artifact: Artifact::new(id, parts),
                append,
                last_chunk,
                metadata: None,
            }))
            .await
    }

    /// Returns `true` if the task has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.ctx.cancellation_token.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::message::{Message, MessageId, MessageRole};
    use a2a_protocol_types::task::TaskId;

    fn make_request_context() -> RequestContext {
        let message = Message {
            id: MessageId::new("test-msg"),
            role: MessageRole::User,
            parts: vec![],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        };
        RequestContext::new(message, TaskId::new("test-task"), "test-ctx".into())
    }

    /// Dummy writer for testing EventEmitter without needing a real queue.
    struct DummyWriter;

    impl EventQueueWriter for DummyWriter {
        fn write<'a>(
            &'a self,
            _event: a2a_protocol_types::events::StreamResponse,
        ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async { Ok(()) })
        }
        fn close<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn is_cancelled_returns_false_initially() {
        let ctx = make_request_context();
        let emit = EventEmitter::new(&ctx, &DummyWriter);
        assert!(!emit.is_cancelled());
    }

    #[test]
    fn is_cancelled_returns_true_after_cancel() {
        let ctx = make_request_context();
        let emit = EventEmitter::new(&ctx, &DummyWriter);
        ctx.cancellation_token.cancel();
        assert!(emit.is_cancelled());
    }
}
