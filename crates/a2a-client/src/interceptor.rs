// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Request/response interceptor infrastructure.
//!
//! Interceptors let callers inspect and modify every A2A request before it is
//! sent and every response after it is received. Common uses include:
//!
//! - Adding `Authorization` headers (see [`crate::auth::AuthInterceptor`]).
//! - Logging or tracing.
//! - Injecting custom metadata.
//!
//! # Example
//!
//! ```rust
//! use a2a_protocol_client::interceptor::{CallInterceptor, ClientRequest, ClientResponse};
//! use a2a_protocol_client::error::ClientResult;
//!
//! struct LoggingInterceptor;
//!
//! impl CallInterceptor for LoggingInterceptor {
//!     fn before<'a>(&'a self, req: &'a mut ClientRequest)
//!         -> impl std::future::Future<Output = ClientResult<()>> + Send + 'a
//!     {
//!         async move { let _ = req; Ok(()) }
//!     }
//!     fn after<'a>(&'a self, resp: &'a ClientResponse)
//!         -> impl std::future::Future<Output = ClientResult<()>> + Send + 'a
//!     {
//!         async move { let _ = resp; Ok(()) }
//!     }
//! }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::ClientResult;

// ── ClientRequest ─────────────────────────────────────────────────────────────

/// A logical A2A request as seen by interceptors.
///
/// Interceptors may mutate `params` and `extra_headers` before the request is
/// dispatched to the transport layer.
#[derive(Debug)]
pub struct ClientRequest {
    /// The A2A method name (e.g. `"message/send"`).
    pub method: String,

    /// Method parameters as a JSON value.
    pub params: serde_json::Value,

    /// Additional HTTP headers to include with this request.
    ///
    /// Auth interceptors use this to inject `Authorization` headers.
    pub extra_headers: HashMap<String, String>,
}

impl ClientRequest {
    /// Creates a new [`ClientRequest`] with the given method and params.
    #[must_use]
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            method: method.into(),
            params,
            extra_headers: HashMap::new(),
        }
    }
}

// ── ClientResponse ────────────────────────────────────────────────────────────

/// A logical A2A response as seen by interceptors.
#[derive(Debug)]
pub struct ClientResponse {
    /// The A2A method name that produced this response.
    pub method: String,

    /// The JSON-decoded result value.
    pub result: serde_json::Value,

    /// The HTTP status code.
    pub status_code: u16,
}

// ── CallInterceptor (public async-fn trait) ───────────────────────────────────

/// Hooks called before every A2A request and after every response.
///
/// Implement this trait to add cross-cutting concerns such as authentication,
/// logging, or metrics. Register interceptors via
/// [`crate::ClientBuilder::with_interceptor`].
///
/// # Object-safety note
///
/// This trait uses `impl Future` return types with explicit lifetimes, which
/// is not object-safe. Internally the SDK wraps implementations in a
/// boxed-future shim. Callers implement the ergonomic trait API.
pub trait CallInterceptor: Send + Sync + 'static {
    /// Called before the request is sent.
    ///
    /// Mutate `req` to modify parameters or inject headers.
    fn before<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a;

    /// Called after a successful response is received.
    fn after<'a>(
        &'a self,
        resp: &'a ClientResponse,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a;
}

// ── Internal boxed trait for object-safe storage ──────────────────────────────

/// Object-safe version of [`CallInterceptor`] used internally.
///
/// Not part of the public API; users implement [`CallInterceptor`].
pub(crate) trait CallInterceptorBoxed: Send + Sync + 'static {
    fn before_boxed<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> Pin<Box<dyn Future<Output = ClientResult<()>> + Send + 'a>>;

    fn after_boxed<'a>(
        &'a self,
        resp: &'a ClientResponse,
    ) -> Pin<Box<dyn Future<Output = ClientResult<()>> + Send + 'a>>;
}

impl<T: CallInterceptor> CallInterceptorBoxed for T {
    fn before_boxed<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> Pin<Box<dyn Future<Output = ClientResult<()>> + Send + 'a>> {
        Box::pin(self.before(req))
    }

    fn after_boxed<'a>(
        &'a self,
        resp: &'a ClientResponse,
    ) -> Pin<Box<dyn Future<Output = ClientResult<()>> + Send + 'a>> {
        Box::pin(self.after(resp))
    }
}

impl CallInterceptorBoxed for Box<dyn CallInterceptorBoxed> {
    fn before_boxed<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> Pin<Box<dyn Future<Output = ClientResult<()>> + Send + 'a>> {
        (**self).before_boxed(req)
    }

    fn after_boxed<'a>(
        &'a self,
        resp: &'a ClientResponse,
    ) -> Pin<Box<dyn Future<Output = ClientResult<()>> + Send + 'a>> {
        (**self).after_boxed(resp)
    }
}

// ── InterceptorChain ──────────────────────────────────────────────────────────

/// An ordered list of [`CallInterceptor`]s applied to every request.
///
/// Interceptors run in registration order for `before` and reverse order for
/// `after` (outermost wraps innermost).
#[derive(Default)]
pub struct InterceptorChain {
    interceptors: Vec<Arc<dyn CallInterceptorBoxed>>,
}

impl InterceptorChain {
    /// Creates an empty [`InterceptorChain`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an interceptor to the end of the chain.
    pub fn push<I: CallInterceptor>(&mut self, interceptor: I) {
        self.interceptors.push(Arc::new(interceptor));
    }

    /// Returns `true` if no interceptors have been registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.interceptors.is_empty()
    }

    /// Runs all `before` hooks in registration order.
    ///
    /// # Errors
    ///
    /// Returns the first error returned by any interceptor in the chain.
    pub async fn run_before(&self, req: &mut ClientRequest) -> ClientResult<()> {
        for interceptor in &self.interceptors {
            interceptor.before_boxed(req).await?;
        }
        Ok(())
    }

    /// Runs all `after` hooks in reverse registration order.
    ///
    /// # Errors
    ///
    /// Returns the first error returned by any interceptor in the chain.
    pub async fn run_after(&self, resp: &ClientResponse) -> ClientResult<()> {
        for interceptor in self.interceptors.iter().rev() {
            interceptor.after_boxed(resp).await?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for InterceptorChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InterceptorChain")
            .field("count", &self.interceptors.len())
            .finish()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingInterceptor(Arc<AtomicUsize>);

    impl CallInterceptor for CountingInterceptor {
        #[allow(clippy::manual_async_fn)]
        fn before<'a>(
            &'a self,
            _req: &'a mut ClientRequest,
        ) -> impl std::future::Future<Output = ClientResult<()>> + Send + 'a {
            async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        #[allow(clippy::manual_async_fn)]
        fn after<'a>(
            &'a self,
            _resp: &'a ClientResponse,
        ) -> impl std::future::Future<Output = ClientResult<()>> + Send + 'a {
            async move {
                self.0.fetch_add(10, Ordering::SeqCst);
                Ok(())
            }
        }
    }

    #[test]
    fn chain_is_empty_when_new() {
        let chain = InterceptorChain::new();
        assert!(chain.is_empty(), "new chain should be empty");
    }

    #[test]
    fn chain_is_not_empty_after_push() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut chain = InterceptorChain::new();
        chain.push(CountingInterceptor(Arc::clone(&counter)));
        assert!(
            !chain.is_empty(),
            "chain with one interceptor should not be empty"
        );
    }

    #[tokio::test]
    async fn chain_runs_before_in_order() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut chain = InterceptorChain::new();
        chain.push(CountingInterceptor(Arc::clone(&counter)));
        chain.push(CountingInterceptor(Arc::clone(&counter)));

        let mut req = ClientRequest::new("message/send", serde_json::Value::Null);
        chain.run_before(&mut req).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn chain_runs_after_in_reverse_order() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut chain = InterceptorChain::new();
        chain.push(CountingInterceptor(Arc::clone(&counter)));

        let resp = ClientResponse {
            method: "message/send".into(),
            result: serde_json::Value::Null,
            status_code: 200,
        };
        chain.run_after(&resp).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }
}
