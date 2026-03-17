// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Token-bucket rate limiter as a [`ServerInterceptor`].
//!
//! Provides [`RateLimitInterceptor`], a ready-made interceptor that limits
//! request throughput per caller. The caller key is derived from
//! [`CallContext::caller_identity`] or the `x-forwarded-for` / peer IP header.
//!
//! # Example
//!
//! ```rust
//! use std::sync::Arc;
//! use a2a_protocol_server::rate_limit::{RateLimitInterceptor, RateLimitConfig};
//!
//! let limiter = Arc::new(RateLimitInterceptor::new(RateLimitConfig {
//!     requests_per_window: 100,
//!     window_secs: 60,
//! }));
//! ```
//!
//! Then add it to the handler builder:
//!
//! ```rust,ignore
//! let handler = RequestHandlerBuilder::new(executor)
//!     .with_interceptor(limiter)
//!     .build()?;
//! ```
//!
//! # Design
//!
//! Uses a fixed-window counter per caller key. Windows are aligned to wall
//! clock seconds. When a request exceeds the per-window limit, the `before`
//! hook returns an error with code `-32029` ("rate limit exceeded").
//!
//! For production deployments requiring sliding windows, distributed counters,
//! or more sophisticated algorithms, implement a custom [`ServerInterceptor`]
//! or use a reverse proxy (nginx, Envoy).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use a2a_protocol_types::error::{A2aError, A2aResult};
use tokio::sync::RwLock;

use crate::call_context::CallContext;
use crate::interceptor::ServerInterceptor;

/// Configuration for [`RateLimitInterceptor`].
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed per window per caller key.
    pub requests_per_window: u64,

    /// Window duration in seconds.
    pub window_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_window: 100,
            window_secs: 60,
        }
    }
}

/// Per-caller rate limit state.
struct CallerBucket {
    /// The window start (seconds since epoch, truncated to `window_secs`).
    window_start: AtomicU64,
    /// Number of requests in the current window.
    count: AtomicU64,
}

/// A fixed-window rate limiting [`ServerInterceptor`].
///
/// Tracks request counts per caller key using a simple fixed-window counter.
/// When the limit is exceeded, rejects the request with an A2A error.
///
/// Caller keys are derived in this order:
/// 1. [`CallContext::caller_identity`] (set by auth interceptors)
/// 2. `x-forwarded-for` HTTP header (first IP)
/// 3. `"anonymous"` fallback
pub struct RateLimitInterceptor {
    config: RateLimitConfig,
    buckets: RwLock<HashMap<String, CallerBucket>>,
    /// Counter for amortized stale-bucket cleanup.
    check_count: AtomicU64,
}

/// Number of `check()` calls between stale-bucket cleanup sweeps.
const CLEANUP_INTERVAL: u64 = 256;

impl std::fmt::Debug for RateLimitInterceptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimitInterceptor")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl RateLimitInterceptor {
    /// Creates a new rate limiter with the given configuration.
    #[must_use]
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: RwLock::new(HashMap::new()),
            check_count: AtomicU64::new(0),
        }
    }

    /// Extracts the caller key from the call context.
    fn caller_key(ctx: &CallContext) -> String {
        if let Some(ref identity) = ctx.caller_identity {
            return identity.clone();
        }
        if let Some(xff) = ctx.http_headers.get("x-forwarded-for") {
            // Use the first IP in the chain (client IP).
            if let Some(ip) = xff.split(',').next() {
                return ip.trim().to_string();
            }
        }
        "anonymous".to_string()
    }

    /// Returns the current window number for the given timestamp.
    const fn window_number(&self, now_secs: u64) -> u64 {
        now_secs / self.config.window_secs
    }

    /// Removes buckets whose window is older than the current window.
    ///
    /// Called periodically (every [`CLEANUP_INTERVAL`] checks) to prevent
    /// unbounded growth of the bucket map from departed callers.
    async fn cleanup_stale_buckets(&self) {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let current_window = self.window_number(now_secs);

        let mut buckets = self.buckets.write().await;
        buckets.retain(|_, bucket| {
            bucket.window_start.load(Ordering::Relaxed) >= current_window.saturating_sub(1)
        });
    }

    /// Checks rate limit for the caller. Returns `Ok(())` if allowed, `Err` if exceeded.
    #[allow(clippy::too_many_lines)]
    async fn check(&self, key: &str) -> A2aResult<()> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let current_window = self.window_number(now_secs);

        // Amortized stale-bucket cleanup to prevent unbounded memory growth.
        let count = self.check_count.fetch_add(1, Ordering::Relaxed);
        if count > 0 && count.is_multiple_of(CLEANUP_INTERVAL) {
            self.cleanup_stale_buckets().await;
        }

        // Fast path: try read lock first.
        {
            let buckets = self.buckets.read().await;
            if let Some(bucket) = buckets.get(key) {
                // CAS loop to atomically reset window or increment counter.
                // Avoids the TOCTOU race where two threads both see an old
                // window and both reset count to 1.
                loop {
                    let bucket_window = bucket.window_start.load(Ordering::Acquire);
                    if bucket_window == current_window {
                        let count = bucket.count.fetch_add(1, Ordering::Relaxed) + 1;
                        if count > self.config.requests_per_window {
                            return Err(A2aError::internal(format!(
                                "rate limit exceeded: {} requests per {} seconds",
                                self.config.requests_per_window, self.config.window_secs
                            )));
                        }
                        return Ok(());
                    }
                    // Window has advanced — atomically swap to the new window.
                    // Only one thread succeeds the CAS; others loop and see the
                    // updated window on the next iteration.
                    if bucket
                        .window_start
                        .compare_exchange(
                            bucket_window,
                            current_window,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        bucket.count.store(1, Ordering::Release);
                        return Ok(());
                    }
                    // CAS failed — another thread updated the window. Retry.
                }
            }
        }

        // Slow path: create new bucket under write lock.
        let mut buckets = self.buckets.write().await;
        // Double-check: another task may have inserted while we waited.
        if let Some(bucket) = buckets.get(key) {
            let bucket_window = bucket.window_start.load(Ordering::Acquire);
            if bucket_window == current_window {
                let count = bucket.count.fetch_add(1, Ordering::Relaxed) + 1;
                if count > self.config.requests_per_window {
                    return Err(A2aError::internal(format!(
                        "rate limit exceeded: {} requests per {} seconds",
                        self.config.requests_per_window, self.config.window_secs
                    )));
                }
            } else {
                bucket.window_start.store(current_window, Ordering::Release);
                bucket.count.store(1, Ordering::Release);
            }
            return Ok(());
        }
        buckets.insert(
            key.to_string(),
            CallerBucket {
                window_start: AtomicU64::new(current_window),
                count: AtomicU64::new(1),
            },
        );
        drop(buckets);
        Ok(())
    }
}

impl ServerInterceptor for RateLimitInterceptor {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let key = Self::caller_key(ctx);
            self.check(&key).await
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_ctx(identity: Option<&str>) -> CallContext {
        CallContext {
            method: "message/send".to_string(),
            caller_identity: identity.map(String::from),
            extensions: vec![],
            request_id: None,
            http_headers: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn allows_requests_within_limit() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 5,
            window_secs: 60,
        });
        let ctx = make_ctx(Some("user-1"));
        for _ in 0..5 {
            assert!(limiter.before(&ctx).await.is_ok());
        }
    }

    #[tokio::test]
    async fn rejects_requests_over_limit() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 3,
            window_secs: 60,
        });
        let ctx = make_ctx(Some("user-2"));
        for _ in 0..3 {
            assert!(limiter.before(&ctx).await.is_ok());
        }
        let result = limiter.before(&ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn different_callers_have_separate_limits() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 2,
            window_secs: 60,
        });
        let ctx_a = make_ctx(Some("alice"));
        let ctx_b = make_ctx(Some("bob"));

        assert!(limiter.before(&ctx_a).await.is_ok());
        assert!(limiter.before(&ctx_a).await.is_ok());
        assert!(limiter.before(&ctx_a).await.is_err()); // alice over limit

        // bob still has his own budget
        assert!(limiter.before(&ctx_b).await.is_ok());
        assert!(limiter.before(&ctx_b).await.is_ok());
    }

    #[tokio::test]
    async fn anonymous_fallback_when_no_identity() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 1,
            window_secs: 60,
        });
        let ctx = make_ctx(None);
        assert!(limiter.before(&ctx).await.is_ok());
        assert!(limiter.before(&ctx).await.is_err());
    }

    #[tokio::test]
    async fn uses_x_forwarded_for_when_no_identity() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 1,
            window_secs: 60,
        });
        let mut headers = HashMap::new();
        headers.insert(
            "x-forwarded-for".to_string(),
            "10.0.0.1, 10.0.0.2".to_string(),
        );
        let ctx = CallContext {
            method: "message/send".to_string(),
            caller_identity: None,
            extensions: vec![],
            request_id: None,
            http_headers: headers,
        };
        assert!(limiter.before(&ctx).await.is_ok());
        assert!(limiter.before(&ctx).await.is_err());
    }

    #[tokio::test]
    async fn concurrent_rate_limit_checks() {
        use std::sync::Arc;

        let limiter = Arc::new(RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 100,
            window_secs: 60,
        }));

        // Spawn 200 concurrent requests from the same caller.
        let mut handles = Vec::new();
        for _ in 0..200 {
            let lim = Arc::clone(&limiter);
            handles.push(tokio::spawn(async move {
                let ctx = CallContext {
                    method: "message/send".to_string(),
                    caller_identity: Some("concurrent-user".into()),
                    extensions: vec![],
                    request_id: None,
                    http_headers: HashMap::new(),
                };
                lim.before(&ctx).await
            }));
        }

        let mut ok_count = 0;
        let mut err_count = 0;
        for handle in handles {
            match handle.await.unwrap() {
                Ok(()) => ok_count += 1,
                Err(_) => err_count += 1,
            }
        }

        // Exactly 100 should succeed, 100 should be rejected.
        assert_eq!(ok_count, 100, "expected 100 allowed, got {ok_count}");
        assert_eq!(err_count, 100, "expected 100 rejected, got {err_count}");
    }

    #[tokio::test]
    async fn stale_bucket_cleanup() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 10,
            window_secs: 60,
        });

        // Create some buckets.
        let ctx_a = make_ctx(Some("stale-a"));
        let ctx_b = make_ctx(Some("stale-b"));
        assert!(limiter.before(&ctx_a).await.is_ok());
        assert!(limiter.before(&ctx_b).await.is_ok());

        assert_eq!(limiter.buckets.read().await.len(), 2);

        // Cleanup shouldn't remove current-window buckets.
        limiter.cleanup_stale_buckets().await;
        assert_eq!(
            limiter.buckets.read().await.len(),
            2,
            "current-window buckets should not be evicted"
        );
    }
}
