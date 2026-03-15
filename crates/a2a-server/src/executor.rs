// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Agent executor trait.
//!
//! [`AgentExecutor`] is the primary extension point for implementing A2A agent
//! logic. The server framework calls [`execute`](AgentExecutor::execute) for
//! every incoming `message/send` or `message/stream` request and
//! [`cancel`](AgentExecutor::cancel) for `tasks/cancel`.

use std::future::Future;
use std::pin::Pin;

use a2a_types::error::A2aResult;

use crate::request_context::RequestContext;
use crate::streaming::EventQueueWriter;

/// Trait for implementing A2A agent execution logic.
///
/// Implementors process incoming messages by writing events (status updates,
/// artifacts) to the provided [`EventQueueWriter`]. The executor runs in a
/// spawned task and should signal completion by writing a terminal status
/// update and returning `Ok(())`.
///
/// # Object safety
///
/// This trait is object-safe: methods return `Pin<Box<dyn Future>>` so that
/// executors can be used as `Arc<dyn AgentExecutor>`. This eliminates the
/// need for generic parameters on [`RequestHandler`](crate::RequestHandler),
/// [`RestDispatcher`](crate::RestDispatcher), and
/// [`JsonRpcDispatcher`](crate::JsonRpcDispatcher), simplifying the entire
/// server API surface.
///
/// # Example
///
/// ```rust,no_run
/// use std::pin::Pin;
/// use std::future::Future;
/// use a2a_server::executor::AgentExecutor;
/// use a2a_server::request_context::RequestContext;
/// use a2a_server::streaming::EventQueueWriter;
/// use a2a_types::error::A2aResult;
///
/// struct MyAgent;
///
/// impl AgentExecutor for MyAgent {
///     fn execute<'a>(
///         &'a self,
///         ctx: &'a RequestContext,
///         queue: &'a dyn EventQueueWriter,
///     ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
///         Box::pin(async move {
///             // Write status updates and artifacts to `queue`.
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait AgentExecutor: Send + Sync + 'static {
    /// Executes agent logic for the given request.
    ///
    /// Write [`StreamResponse`](a2a_types::events::StreamResponse) events to
    /// `queue` as the agent progresses. The method should return `Ok(())`
    /// after writing the final event, or `Err(...)` on failure.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if execution fails.
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Cancels an in-progress task.
    ///
    /// The default implementation returns an error indicating the task is not
    /// cancelable. Override this to support task cancellation.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if cancellation fails
    /// or is not supported.
    fn cancel<'a>(
        &'a self,
        ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            Err(a2a_types::error::A2aError::task_not_cancelable(
                &ctx.task_id,
            ))
        })
    }
}
