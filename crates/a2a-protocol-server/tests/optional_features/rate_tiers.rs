// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §13.4: "Agents MAY implement different rate limits for different
//! operations or user tiers." Tiers are tenants with their own
//! `rate_limit_rps`: under one limiter, a premium tenant's callers get more
//! requests per window than a free tenant's.

use std::time::{SystemTime, UNIX_EPOCH};

use a2a_protocol_server::{
    CallContext, PerTenantConfig, RateLimitConfig, RateLimitInterceptor, ServerInterceptor,
    TenantContext, TenantLimits,
};

const WINDOW_SECS: u64 = 60;

fn window_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / WINDOW_SECS
}

/// One limiter: premium is 3 requests per second, free is 1, and the
/// per-caller limit is high enough never to be the one that refuses.
fn limiter() -> RateLimitInterceptor {
    RateLimitInterceptor::new(
        RateLimitConfig::default()
            .with_requests_per_window(1_000_000)
            .with_window_secs(WINDOW_SECS),
    )
    .unwrap()
    .with_tenant_config(
        PerTenantConfig::builder()
            .with_override("premium", TenantLimits::builder().rate_limit_rps(3).build())
            .with_override("free", TenantLimits::builder().rate_limit_rps(1).build())
            .build(),
    )
}

/// How many requests `tenant`'s caller makes before the first refusal, out
/// of at most `cap`, within one window. A run that straddles a window
/// boundary is retried, so the count is never split across two windows.
async fn admitted(tenant: &'static str, cap: u64) -> u64 {
    for _ in 0..3 {
        let start = window_now();
        let rl = limiter();
        let ctx = CallContext::new("SendMessage").with_caller_identity(format!("{tenant}-user"));
        let n = TenantContext::scope(tenant, async {
            let mut n = 0;
            while n < cap && rl.before(&ctx).await.is_ok() {
                n += 1;
            }
            n
        })
        .await;
        if window_now() == start {
            return n;
        }
    }
    panic!("three runs each straddled a {WINDOW_SECS}s window boundary");
}

#[tokio::test]
async fn tenants_on_different_tiers_get_different_rate_limits() {
    assert_eq!(admitted("free", 1_000).await, WINDOW_SECS, "1 rps x 60s");
    assert_eq!(
        admitted("premium", 1_000).await,
        3 * WINDOW_SECS,
        "3 rps x 60s"
    );
}
