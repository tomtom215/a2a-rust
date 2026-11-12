// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An identity provider that is down must be asked once, not once per request
//! (adversarial review R4, 2026-11-12).
//!
//! With an empty or expired key cache, `RemoteJwks::get` serialised callers on
//! a lock and let each in turn fetch: a failed fetch leaves no fresh cache, so
//! every waiter made its own fetch to a provider that had just failed, each
//! waiting out the provider's full budget in turn. These tests drive a real
//! provider (a loopback socket) that fails fast, stalls, and recovers, with
//! many concurrent callers presenting syntactically valid tokens that reach
//! the key lookup.

#![cfg(feature = "auth-jwt")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use a2a_protocol_server::auth::jwt::{JwtAuthInterceptor, JwtValidator};
use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::interceptor::ServerInterceptor;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[allow(dead_code)]
mod vectors {
    include!("../src/auth/jwt_test_vectors.rs");
}
use vectors::{RS256_E, RS256_N, RS256_WRONG_KEY};

/// What the provider does with a request.
const FAIL: usize = 0;
const STALL: usize = 1;
const SERVE: usize = 2;

struct Provider {
    url: String,
    mode: Arc<AtomicUsize>,
    /// Requests the provider has accepted.
    fetches: Arc<AtomicUsize>,
}

async fn provider(initial: usize) -> Provider {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mode = Arc::new(AtomicUsize::new(initial));
    let fetches = Arc::new(AtomicUsize::new(0));
    let (m, f) = (Arc::clone(&mode), Arc::clone(&fetches));
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let (m, f) = (Arc::clone(&m), Arc::clone(&f));
            tokio::spawn(async move {
                let mut buf = [0_u8; 2048];
                let _ = sock.read(&mut buf).await;
                f.fetch_add(1, Ordering::SeqCst);
                let reply = match m.load(Ordering::SeqCst) {
                    STALL => std::future::pending().await,
                    SERVE => {
                        let body = format!(
                            r#"{{"keys":[{{"kty":"RSA","kid":"rk1","use":"sig","n":"{RS256_N}","e":"{RS256_E}"}}]}}"#
                        );
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    _ => "HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
                };
                let _ = sock.write_all(reply.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Provider {
        url: format!("http://127.0.0.1:{}/jwks", addr.port()),
        mode,
        fetches,
    }
}

fn validator() -> JwtValidator {
    JwtValidator::new()
        .with_issuer("https://issuer.test")
        .with_audience("a2a-agent")
}

fn ctx() -> CallContext {
    let mut headers = std::collections::HashMap::new();
    headers.insert(
        "authorization".to_owned(),
        format!("Bearer {RS256_WRONG_KEY}"),
    );
    CallContext::new("SendMessage").with_http_headers(headers)
}

/// 100 concurrent requests against a provider that answers 503 at once, with a
/// cold cache: one fetch per cooldown, not one per request. Every request is
/// still refused (fail closed), and quickly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_provider_that_fails_is_asked_once_per_cooldown_not_once_per_request() {
    let p = provider(FAIL).await;
    let interceptor = Arc::new(
        JwtAuthInterceptor::from_jwks_url(validator(), &p.url)
            .with_jwks_refetch_interval(Duration::from_secs(30)),
    );
    let started = Instant::now();
    let calls: Vec<_> = (0..100)
        .map(|_| {
            let interceptor = Arc::clone(&interceptor);
            tokio::spawn(async move { interceptor.before(&ctx()).await })
        })
        .collect();
    for call in calls {
        assert!(
            call.await.unwrap().is_err(),
            "a failed key fetch fails closed"
        );
    }
    let fetches = p.fetches.load(Ordering::SeqCst);
    assert!(
        fetches <= 2,
        "{fetches} fetches for 100 concurrent requests to a provider that had just failed"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );

    // And later requests, still inside the cooldown, do not fetch either.
    for _ in 0..20 {
        assert!(interceptor.before(&ctx()).await.is_err());
    }
    assert!(
        p.fetches.load(Ordering::SeqCst) <= fetches,
        "a request inside the cooldown fetched again"
    );
}

/// When the provider comes back, the first request after the cooldown fetches,
/// the keys are cached, and nothing is fetched again within their lifetime.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_provider_is_asked_again_after_the_cooldown_and_recovery_is_seen() {
    let p = provider(FAIL).await;
    let interceptor = JwtAuthInterceptor::from_jwks_url(validator(), &p.url)
        .with_jwks_refetch_interval(Duration::from_millis(300));
    let first = interceptor.before(&ctx()).await.expect_err("provider down");
    assert!(
        first.auth_rejection().is_none(),
        "a dependency failure, not a rejection: {first:?}"
    );

    p.mode.store(SERVE, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(450)).await;
    let wrong_key = interceptor.before(&ctx()).await.expect_err("wrong key");
    assert!(
        wrong_key.auth_rejection().is_some(),
        "keys were fetched, so the token is refused as a token: {wrong_key:?}"
    );
    let after_recovery = p.fetches.load(Ordering::SeqCst);
    for _ in 0..20 {
        let _ = interceptor.before(&ctx()).await;
    }
    assert_eq!(
        p.fetches.load(Ordering::SeqCst),
        after_recovery,
        "the keys are cached"
    );
}

/// A provider that accepts the connection and never answers: the first request
/// holds the fetch for the whole budget, and no request behind it waits more
/// than about that, fetches again, or is let through unvalidated.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_provider_that_stalls_costs_each_request_at_most_one_budget() {
    let p = provider(STALL).await;
    let interceptor = Arc::new(
        JwtAuthInterceptor::from_jwks_url(validator(), &p.url)
            .with_jwks_refetch_interval(Duration::from_secs(30))
            .with_jwks_fetch_budget(Duration::from_secs(1)),
    );
    let started = Instant::now();
    let calls: Vec<_> = (0..100)
        .map(|_| {
            let interceptor = Arc::clone(&interceptor);
            tokio::spawn(async move {
                let outcome = interceptor.before(&ctx()).await;
                (outcome.is_err(), started.elapsed())
            })
        })
        .collect();
    let mut slowest = Duration::ZERO;
    for call in calls {
        let (refused, took) = call.await.unwrap();
        assert!(refused, "no keys, no validation");
        slowest = slowest.max(took);
    }
    assert!(
        slowest < Duration::from_secs(4),
        "the slowest request waited {slowest:?} behind a stalled provider (budget 1 s)"
    );
    let fetches = p.fetches.load(Ordering::SeqCst);
    assert!(fetches <= 2, "{fetches} fetches to a provider that stalled");
}

/// The queue behind a stalled fetch is bounded: requests past it are refused at
/// once, not parked for the budget.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_past_the_waiter_bound_are_refused_at_once() {
    let p = provider(STALL).await;
    let interceptor = Arc::new(
        JwtAuthInterceptor::from_jwks_url(validator(), &p.url)
            .with_jwks_refetch_interval(Duration::from_secs(30))
            .with_jwks_fetch_budget(Duration::from_secs(3)),
    );
    let started = Instant::now();
    let early = Arc::new(AtomicUsize::new(0));
    let calls: Vec<_> = (0..400)
        .map(|_| {
            let (interceptor, early) = (Arc::clone(&interceptor), Arc::clone(&early));
            tokio::spawn(async move {
                let _ = interceptor.before(&ctx()).await;
                if started.elapsed() < Duration::from_millis(1_000) {
                    early.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    let refused_early = early.load(Ordering::SeqCst);
    assert!(
        refused_early >= 100,
        "{refused_early} of 400 requests were refused within a second; the others were queued without bound"
    );
    for call in calls {
        call.await.unwrap();
    }
}

/// An operator who sets the cooldown to zero asked for a fetch per request; each
/// request is still bounded by its own deadline, so the queue behind a stalled
/// fetch is not k times the budget.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_no_cooldown_each_request_is_still_bounded_by_the_budget() {
    let p = provider(STALL).await;
    let interceptor = Arc::new(
        JwtAuthInterceptor::from_jwks_url(validator(), &p.url)
            .with_jwks_refetch_interval(Duration::ZERO)
            .with_jwks_fetch_budget(Duration::from_secs(1)),
    );
    let started = Instant::now();
    let calls: Vec<_> = (0..6)
        .map(|_| {
            let interceptor = Arc::clone(&interceptor);
            tokio::spawn(async move { interceptor.before(&ctx()).await.is_err() })
        })
        .collect();
    for call in calls {
        assert!(call.await.unwrap());
    }
    assert!(
        started.elapsed() < Duration::from_millis(3_500),
        "six requests behind stalled fetches took {:?}: queued one budget after another",
        started.elapsed()
    );
}
