// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for the ServerInterceptorChain.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use a2a_types::error::{A2aError, A2aResult, ErrorCode};

use a2a_server::interceptor::{ServerInterceptor, ServerInterceptorChain};
use a2a_server::CallContext;

// ── Helpers ──────────────────────────────────────────────────────────────────

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

/// Interceptor that rejects all requests in `before`.
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

/// Interceptor that fails in `after`.
struct AfterFailInterceptor;

impl ServerInterceptor for AfterFailInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Err(A2aError::new(ErrorCode::InternalError, "after failed")) })
    }
}

/// Interceptor that records its label into a shared log on each call,
/// so we can verify ordering.
struct OrderRecordingInterceptor {
    label: String,
    log: Arc<Mutex<Vec<String>>>,
}

impl OrderRecordingInterceptor {
    fn new(label: impl Into<String>, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            label: label.into(),
            log,
        }
    }
}

impl ServerInterceptor for OrderRecordingInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.log
                .lock()
                .unwrap()
                .push(format!("before:{}", self.label));
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.log
                .lock()
                .unwrap()
                .push(format!("after:{}", self.label));
            Ok(())
        })
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

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
async fn multiple_interceptors_ordering_preserved() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));

    let mut chain = ServerInterceptorChain::new();
    chain.push(Arc::new(OrderRecordingInterceptor::new(
        "A",
        Arc::clone(&log),
    )));
    chain.push(Arc::new(OrderRecordingInterceptor::new(
        "B",
        Arc::clone(&log),
    )));
    chain.push(Arc::new(OrderRecordingInterceptor::new(
        "C",
        Arc::clone(&log),
    )));

    let ctx = CallContext::new("TestMethod");

    chain.run_before(&ctx).await.unwrap();
    chain.run_after(&ctx).await.unwrap();

    let entries = log.lock().unwrap().clone();
    // before runs in insertion order: A, B, C
    assert_eq!(entries[0], "before:A");
    assert_eq!(entries[1], "before:B");
    assert_eq!(entries[2], "before:C");
    // after runs in reverse order: C, B, A
    assert_eq!(entries[3], "after:C");
    assert_eq!(entries[4], "after:B");
    assert_eq!(entries[5], "after:A");
}

#[tokio::test]
async fn error_in_before_stops_chain() {
    let counter = Arc::new(CountingInterceptor::new());
    let mut chain = ServerInterceptorChain::new();
    chain.push(Arc::new(RejectingInterceptor));
    chain.push(Arc::clone(&counter) as Arc<dyn ServerInterceptor>);

    let ctx = CallContext::new("TestMethod");

    let result = chain.run_before(&ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedOperation);
    assert_eq!(err.message, "rejected");

    // The counting interceptor should NOT have been called (short-circuit).
    assert_eq!(counter.before_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn error_in_after_propagated() {
    let mut chain = ServerInterceptorChain::new();
    chain.push(Arc::new(AfterFailInterceptor));

    let ctx = CallContext::new("TestMethod");

    // before succeeds.
    chain.run_before(&ctx).await.unwrap();

    // after returns the error from the interceptor.
    let result = chain.run_after(&ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::InternalError);
    assert_eq!(err.message, "after failed");
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
