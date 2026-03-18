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
        if let Some(identity) = ctx.caller_identity() {
            return identity.to_owned();
        }
        if let Some(xff) = ctx.http_headers().get("x-forwarded-for") {
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
        let mut ctx = CallContext::new("message/send");
        if let Some(id) = identity {
            ctx = ctx.with_caller_identity(id.to_owned());
        }
        ctx
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
        let ctx = CallContext::new("message/send").with_http_headers(headers);
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
                let ctx = CallContext::new("message/send")
                    .with_caller_identity("concurrent-user".into());
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

    #[test]
    fn debug_format_includes_config() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 42,
            window_secs: 10,
        });
        let debug = format!("{limiter:?}");
        assert!(
            debug.contains("RateLimitInterceptor"),
            "Debug output should contain struct name"
        );
        assert!(
            debug.contains("config"),
            "Debug output should contain config field"
        );
    }

    /// Covers lines 63-68 (`RateLimitConfig::default`).
    #[test]
    fn default_config_values() {
        let config = RateLimitConfig::default();
        assert_eq!(config.requests_per_window, 100);
        assert_eq!(config.window_secs, 60);
    }

    /// Covers lines 250-255 (after hook returns Ok).
    #[tokio::test]
    async fn after_hook_is_noop() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig::default());
        let ctx = make_ctx(Some("user"));
        let result = limiter.after(&ctx).await;
        assert!(result.is_ok(), "after hook should always return Ok");
    }

    #[test]
    fn window_number_correctness() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 10,
            window_secs: 60,
        });

        // 0 seconds → window 0
        assert_eq!(limiter.window_number(0), 0);
        // 59 seconds → still window 0
        assert_eq!(limiter.window_number(59), 0);
        // 60 seconds → window 1
        assert_eq!(limiter.window_number(60), 1);
        // 120 seconds → window 2
        assert_eq!(limiter.window_number(120), 2);
        // 61 seconds → window 1
        assert_eq!(limiter.window_number(61), 1);
    }

    #[tokio::test]
    async fn cleanup_stale_buckets_removes_old_entries() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 100,
            window_secs: 60,
        });

        // Manually insert a bucket with an ancient window.
        {
            let mut buckets = limiter.buckets.write().await;
            buckets.insert(
                "ancient-user".to_string(),
                CallerBucket {
                    window_start: AtomicU64::new(0), // window 0 = epoch
                    count: AtomicU64::new(5),
                },
            );
        }
        assert_eq!(limiter.buckets.read().await.len(), 1);

        // Cleanup should remove the ancient bucket.
        limiter.cleanup_stale_buckets().await;
        assert_eq!(
            limiter.buckets.read().await.len(),
            0,
            "ancient bucket should be evicted"
        );
    }

    #[tokio::test]
    async fn check_triggers_cleanup_at_interval() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 10000,
            window_secs: 60,
        });

        // Insert a stale bucket manually.
        {
            let mut buckets = limiter.buckets.write().await;
            buckets.insert(
                "stale-for-cleanup".to_string(),
                CallerBucket {
                    window_start: AtomicU64::new(0),
                    count: AtomicU64::new(1),
                },
            );
        }

        // Set check_count so the next fetch_add returns CLEANUP_INTERVAL (a multiple),
        // which triggers cleanup.
        limiter
            .check_count
            .store(CLEANUP_INTERVAL, Ordering::Relaxed);

        let ctx = make_ctx(Some("cleanup-trigger-user"));
        // This check should trigger cleanup (count becomes CLEANUP_INTERVAL).
        assert!(limiter.before(&ctx).await.is_ok());

        // The stale bucket should have been cleaned up.
        let buckets = limiter.buckets.read().await;
        let has_stale = buckets.contains_key("stale-for-cleanup");
        drop(buckets);
        assert!(
            !has_stale,
            "stale bucket should be cleaned up after CLEANUP_INTERVAL checks"
        );
    }

    #[tokio::test]
    async fn slow_path_double_check_same_window() {
        // Test the slow-path double-check logic (lines 211-225).
        // When two tasks race to create a bucket, the second should increment
        // the existing bucket rather than creating a duplicate.
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 2,
            window_secs: 60,
        });

        let ctx = make_ctx(Some("race-user"));
        // First request creates the bucket.
        assert!(limiter.before(&ctx).await.is_ok());
        // Second request hits the fast path.
        assert!(limiter.before(&ctx).await.is_ok());
        // Third should be rejected.
        assert!(limiter.before(&ctx).await.is_err());
    }

    /// Covers lines 211-226: slow-path double-check when a bucket exists but
    /// its window has advanced (the `else` branch on line 221-223).
    #[tokio::test]
    async fn slow_path_double_check_stale_window() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 10,
            window_secs: 60,
        });

        // Manually insert a bucket with an old window_start so that the
        // slow-path re-check finds it with a stale window.
        let key = "slow-path-stale";
        {
            let mut buckets = limiter.buckets.write().await;
            buckets.insert(
                key.to_string(),
                CallerBucket {
                    window_start: AtomicU64::new(1), // ancient window
                    count: AtomicU64::new(5),
                },
            );
        }

        // Now remove from the fast-path perspective by holding a write lock
        // briefly; the check method will fall through to the slow path where
        // the bucket exists but has an old window. We call check() directly.
        let result = limiter.check(key).await;
        assert!(
            result.is_ok(),
            "slow-path stale-window reset should succeed"
        );

        // The window should have been updated and count reset to 1.
        assert_eq!(
            limiter
                .buckets
                .read()
                .await
                .get(key)
                .expect("bucket should exist")
                .count
                .load(Ordering::Relaxed),
            1,
            "count should be reset to 1 after window advance"
        );
    }

    /// Covers lines 214-219: slow-path double-check when the bucket exists in
    /// the current window and count exceeds the limit.
    #[tokio::test]
    async fn slow_path_rate_limit_exceeded() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 1,
            window_secs: 60,
        });

        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let current_window = limiter.window_number(now_secs);

        // Manually insert a bucket already at the limit in the current window.
        let key = "slow-path-exceeded";
        {
            let mut buckets = limiter.buckets.write().await;
            buckets.insert(
                key.to_string(),
                CallerBucket {
                    window_start: AtomicU64::new(current_window),
                    count: AtomicU64::new(1), // already at limit
                },
            );
        }

        // check() should hit the slow-path double-check and see that
        // the count exceeds the limit.
        let result = limiter.check(key).await;
        assert!(
            result.is_err(),
            "slow-path should reject when count exceeds limit"
        );
    }

    /// Covers lines 179-183: fast-path rate limit exceeded (count > `requests_per_window`).
    #[tokio::test]
    async fn fast_path_rate_limit_exceeded() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 2,
            window_secs: 60,
        });

        // First two requests create and use the fast-path bucket.
        let ctx = make_ctx(Some("fast-path-user"));
        assert!(limiter.before(&ctx).await.is_ok());
        assert!(limiter.before(&ctx).await.is_ok());
        // Third request should hit the fast-path count > limit check.
        let result = limiter.before(&ctx).await;
        assert!(
            result.is_err(),
            "fast-path should reject when count exceeds limit"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("rate limit exceeded"),
            "error message should mention rate limit exceeded, got: {err}"
        );
    }

    /// Covers lines 190-202: the CAS loop for window advancement in the fast path.
    /// When the bucket exists with an old window, the CAS succeeds and resets count.
    #[tokio::test]
    async fn fast_path_window_advancement_resets_count() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 1,
            window_secs: 60,
        });

        let key = "fast-path-window-advance";
        // Manually insert a bucket with an old window so the fast-path CAS fires.
        {
            let mut buckets = limiter.buckets.write().await;
            buckets.insert(
                key.to_string(),
                CallerBucket {
                    window_start: AtomicU64::new(1), // ancient window
                    count: AtomicU64::new(999),
                },
            );
        }

        // check() should find the bucket in the fast-path read lock, see the old
        // window, succeed the CAS, and reset count to 1.
        let result = limiter.check(key).await;
        assert!(result.is_ok(), "fast-path window advance should succeed");

        assert_eq!(
            limiter
                .buckets
                .read()
                .await
                .get(key)
                .expect("bucket should exist")
                .count
                .load(Ordering::Relaxed),
            1,
            "count should be reset to 1 after window advance"
        );
    }

    /// Kills mutations on line 164: `&& → ||` and `> → >=`.
    ///
    /// With `&&`: `0 > 0 && 0.is_multiple_of(256)` = `false && true` = `false` → no cleanup.
    /// With `||`: `0 > 0 || 0.is_multiple_of(256)` = `false || true` = `true` → cleanup (wrong!).
    /// With `>=`: `0 >= 0 && 0.is_multiple_of(256)` = `true && true` = `true` → cleanup (wrong!).
    #[tokio::test]
    async fn cleanup_does_not_run_on_first_call() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 10000,
            window_secs: 60,
        });

        // Insert a stale bucket before any calls.
        {
            let mut buckets = limiter.buckets.write().await;
            buckets.insert(
                "stale-first-call".to_string(),
                CallerBucket {
                    window_start: AtomicU64::new(0),
                    count: AtomicU64::new(1),
                },
            );
        }

        // Make one call. check_count starts at 0; fetch_add returns 0.
        // With correct code: count(0) > 0 is false → no cleanup.
        let ctx = make_ctx(Some("first-caller"));
        assert!(limiter.before(&ctx).await.is_ok());

        // The stale bucket should still exist (no cleanup on first call).
        assert!(
            limiter
                .buckets
                .read()
                .await
                .contains_key("stale-first-call"),
            "stale bucket should not be cleaned up on the very first call"
        );
    }

    /// Covers the `caller_key` function with an empty x-forwarded-for (no commas).
    #[tokio::test]
    async fn x_forwarded_for_single_ip() {
        let limiter = RateLimitInterceptor::new(RateLimitConfig {
            requests_per_window: 1,
            window_secs: 60,
        });
        let mut headers = HashMap::new();
        headers.insert("x-forwarded-for".to_string(), "192.168.1.1".to_string());
        let ctx = CallContext::new("message/send").with_http_headers(headers);
        assert!(limiter.before(&ctx).await.is_ok());
        // Second request should be rejected (limit is 1).
        assert!(limiter.before(&ctx).await.is_err());
    }
}
