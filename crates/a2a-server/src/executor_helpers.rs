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
