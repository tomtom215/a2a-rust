// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Server-side interceptor chain.
//!
//! [`ServerInterceptor`] allows middleware-style hooks before and after each
//! JSON-RPC or REST method invocation. [`ServerInterceptorChain`] manages an
//! ordered list of interceptors and runs them sequentially.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::error::A2aResult;

use crate::call_context::CallContext;

/// A server-side interceptor for request processing.
///
/// Interceptors run before and after the core handler logic. They can be used
/// for logging, authentication, rate-limiting, or other cross-cutting concerns.
///
/// # Object safety
///
/// This trait is designed to be used behind `Arc<dyn ServerInterceptor>`.
pub trait ServerInterceptor: Send + Sync + 'static {
    /// Called before the request handler processes the method call.
    ///
    /// Return `Err(...)` to abort the request with an error response.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) to reject the request.
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Called after the request handler has finished processing.
    ///
    /// This is called even if the handler returned an error. It should not
    /// alter the response — use it for logging, metrics, or cleanup.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if post-processing fails.
    fn after<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
}

/// An ordered chain of [`ServerInterceptor`] instances.
///
/// Interceptors are executed in insertion order for `before` and reverse order
/// for `after`.
#[derive(Default)]
pub struct ServerInterceptorChain {
    interceptors: Vec<Arc<dyn ServerInterceptor>>,
}

impl ServerInterceptorChain {
    /// Creates an empty interceptor chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an interceptor to the chain.
    pub fn push(&mut self, interceptor: Arc<dyn ServerInterceptor>) {
        self.interceptors.push(interceptor);
    }

    /// Runs all `before` hooks in insertion order.
    ///
    /// Stops at the first error and returns it.
    ///
    /// # Errors
    ///
    /// Returns the first [`A2aError`](a2a_protocol_types::error::A2aError) from any interceptor.
    pub async fn run_before(&self, ctx: &CallContext) -> A2aResult<()> {
        for interceptor in &self.interceptors {
            interceptor.before(ctx).await?;
        }
        Ok(())
    }

    /// Runs all `after` hooks in reverse insertion order.
    ///
    /// Stops at the first error and returns it.
    ///
    /// # Errors
    ///
    /// Returns the first [`A2aError`](a2a_protocol_types::error::A2aError) from any interceptor.
    pub async fn run_after(&self, ctx: &CallContext) -> A2aResult<()> {
        for interceptor in self.interceptors.iter().rev() {
            interceptor.after(ctx).await?;
        }
        Ok(())
    }
}

impl fmt::Debug for ServerInterceptorChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerInterceptorChain")
            .field("count", &self.interceptors.len())
            .finish()
    }
}

use std::fmt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_shows_count() {
        let chain = ServerInterceptorChain::new();
        let debug = format!("{chain:?}");
        assert!(debug.contains("ServerInterceptorChain"));
        assert!(debug.contains("count"));
        assert!(debug.contains('0'));
    }

    struct NoopInterceptor;
    impl ServerInterceptor for NoopInterceptor {
        fn before<'a>(
            &'a self,
            _ctx: &'a CallContext,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
        fn after<'a>(
            &'a self,
            _ctx: &'a CallContext,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn debug_shows_correct_count_after_push() {
        let mut chain = ServerInterceptorChain::new();
        chain.push(Arc::new(NoopInterceptor));
        chain.push(Arc::new(NoopInterceptor));
        let debug = format!("{chain:?}");
        assert!(debug.contains('2'), "expected count=2 in debug: {debug}");
    }
}
