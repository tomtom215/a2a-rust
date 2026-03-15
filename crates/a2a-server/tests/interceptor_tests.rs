// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for the ServerInterceptorChain.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use a2a_types::error::{A2aError, A2aResult, ErrorCode};

use a2a_server::interceptor::{ServerInterceptor, ServerInterceptorChain};
use a2a_server::CallContext;

/// Interceptor that counts calls.
struct CountingInterceptor {
    before_count: AtomicU32,
    after_count: AtomicU32,
}

impl CountingInterceptor {
    fn new() -> Self {
        Self {
            before_count: AtomicU32::new(0),
            after_count: AtomicU32::new(0),
        }
    }
}

impl ServerInterceptor for CountingInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.before_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.after_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

/// Interceptor that rejects all requests.
struct RejectingInterceptor;

impl ServerInterceptor for RejectingInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Err(A2aError::new(ErrorCode::UnsupportedOperation, "rejected")) })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

#[tokio::test]
async fn empty_chain_passes() {
    let chain = ServerInterceptorChain::new();
    let ctx = CallContext::new("TestMethod");
    chain.run_before(&ctx).await.unwrap();
    chain.run_after(&ctx).await.unwrap();
}

#[tokio::test]
async fn interceptor_before_and_after_called() {
    let interceptor = Arc::new(CountingInterceptor::new());
    let mut chain = ServerInterceptorChain::new();
    chain.push(Arc::clone(&interceptor) as Arc<dyn ServerInterceptor>);

    let ctx = CallContext::new("TestMethod");

    chain.run_before(&ctx).await.unwrap();
    assert_eq!(interceptor.before_count.load(Ordering::SeqCst), 1);

    chain.run_after(&ctx).await.unwrap();
    assert_eq!(interceptor.after_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rejecting_interceptor_short_circuits_before() {
    let counter = Arc::new(CountingInterceptor::new());
    let mut chain = ServerInterceptorChain::new();
    chain.push(Arc::new(RejectingInterceptor));
    chain.push(Arc::clone(&counter) as Arc<dyn ServerInterceptor>);

    let ctx = CallContext::new("TestMethod");

    let result = chain.run_before(&ctx).await;
    assert!(result.is_err());

    // The counting interceptor should NOT have been called (short-circuit)
    assert_eq!(counter.before_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn after_runs_in_reverse_order() {
    let first = Arc::new(CountingInterceptor::new());
    let second = Arc::new(CountingInterceptor::new());

    let mut chain = ServerInterceptorChain::new();
    chain.push(Arc::clone(&first) as Arc<dyn ServerInterceptor>);
    chain.push(Arc::clone(&second) as Arc<dyn ServerInterceptor>);

    let ctx = CallContext::new("TestMethod");

    // Both should be called for before (in order)
    chain.run_before(&ctx).await.unwrap();
    assert_eq!(first.before_count.load(Ordering::SeqCst), 1);
    assert_eq!(second.before_count.load(Ordering::SeqCst), 1);

    // Both should be called for after (in reverse order)
    chain.run_after(&ctx).await.unwrap();
    assert_eq!(first.after_count.load(Ordering::SeqCst), 1);
    assert_eq!(second.after_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn call_context_builder() {
    let ctx = CallContext::new("SendMessage")
        .with_caller_identity("user-123".into())
        .with_extensions(vec!["ext-1".into()]);

    assert_eq!(ctx.method, "SendMessage");
    assert_eq!(ctx.caller_identity, Some("user-123".into()));
    assert_eq!(ctx.extensions, vec!["ext-1".to_owned()]);
}
