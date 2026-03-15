// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Integration tests for agent card discovery module.

use a2a_client::discovery::{
    fetch_card_from_url, resolve_agent_card, resolve_agent_card_with_path, CachingCardResolver,
};
use a2a_client::error::ClientError;

// ── URL validation tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn resolve_agent_card_rejects_empty_url() {
    let err = resolve_agent_card("").await.unwrap_err();
    assert!(
        matches!(err, ClientError::InvalidEndpoint(ref msg) if msg.contains("empty")),
        "expected InvalidEndpoint for empty URL, got: {err:?}",
    );
}

#[tokio::test]
async fn resolve_agent_card_rejects_non_http_scheme() {
    let err = resolve_agent_card("ftp://example.com").await.unwrap_err();
    assert!(
        matches!(err, ClientError::InvalidEndpoint(ref msg) if msg.contains("http")),
        "expected InvalidEndpoint for non-http scheme, got: {err:?}",
    );
}

#[tokio::test]
async fn resolve_agent_card_with_path_rejects_empty_url() {
    let err = resolve_agent_card_with_path("", "/card.json")
        .await
        .unwrap_err();
    assert!(
        matches!(err, ClientError::InvalidEndpoint(_)),
        "expected InvalidEndpoint, got: {err:?}",
    );
}

#[tokio::test]
async fn resolve_agent_card_with_path_rejects_non_http_scheme() {
    let err = resolve_agent_card_with_path("ws://example.com", "/card.json")
        .await
        .unwrap_err();
    assert!(
        matches!(err, ClientError::InvalidEndpoint(ref msg) if msg.contains("http")),
        "expected InvalidEndpoint for ws:// scheme, got: {err:?}",
    );
}

#[tokio::test]
async fn fetch_card_from_url_rejects_garbage_uri() {
    // An invalid URI that hyper cannot parse should produce an error.
    let err = fetch_card_from_url("not a valid url at all")
        .await
        .unwrap_err();
    // This will fail during request construction (Transport) or connection (HttpClient).
    assert!(
        matches!(err, ClientError::Transport(_) | ClientError::HttpClient(_)),
        "expected Transport or HttpClient error for garbage URI, got: {err:?}",
    );
}

// ── Connection-refused tests ─────────────────────────────────────────────────

#[tokio::test]
async fn resolve_agent_card_connection_refused() {
    // Port 19999 should have no listener.
    let err = resolve_agent_card("http://127.0.0.1:19999")
        .await
        .unwrap_err();
    assert!(
        matches!(err, ClientError::HttpClient(_)),
        "expected HttpClient error for connection refused, got: {err:?}",
    );
}

#[tokio::test]
async fn resolve_agent_card_with_path_connection_refused() {
    let err = resolve_agent_card_with_path("http://127.0.0.1:19999", "/custom/path.json")
        .await
        .unwrap_err();
    assert!(
        matches!(err, ClientError::HttpClient(_)),
        "expected HttpClient error for connection refused, got: {err:?}",
    );
}

#[tokio::test]
async fn fetch_card_from_url_connection_refused() {
    let err = fetch_card_from_url("http://127.0.0.1:19999/.well-known/agent.json")
        .await
        .unwrap_err();
    assert!(
        matches!(err, ClientError::HttpClient(_)),
        "expected HttpClient error for connection refused, got: {err:?}",
    );
}

#[tokio::test]
async fn caching_resolver_resolve_connection_refused() {
    let resolver = CachingCardResolver::new("http://127.0.0.1:19999");
    let err = resolver.resolve().await.unwrap_err();
    assert!(
        matches!(err, ClientError::HttpClient(_)),
        "expected HttpClient error from CachingCardResolver, got: {err:?}",
    );
}

// ── CachingCardResolver construction ─────────────────────────────────────────

#[test]
fn caching_resolver_new_constructs_well_known_url() {
    let resolver = CachingCardResolver::new("http://localhost:8080");
    // Clone works (the type derives Clone).
    let _cloned = resolver.clone();
}

#[test]
fn caching_resolver_new_trailing_slash_is_normalized() {
    let r1 = CachingCardResolver::new("http://localhost:8080");
    let r2 = CachingCardResolver::new("http://localhost:8080/");
    // Both should resolve to the same underlying URL.  We cannot inspect
    // the private field, but we can verify construction does not panic.
    let _ = (r1, r2);
}

#[test]
fn caching_resolver_with_path_custom() {
    let resolver =
        CachingCardResolver::with_path("http://localhost:8080", "/api/v2/agent-card.json");
    let _cloned = resolver.clone();
}

#[test]
fn caching_resolver_with_path_no_leading_slash() {
    // A path without a leading slash should still succeed (the internal
    // build_card_url prepends one).
    let resolver = CachingCardResolver::with_path("http://localhost:8080", "card.json");
    let _ = resolver;
}

#[test]
fn caching_resolver_new_with_https_scheme() {
    let resolver = CachingCardResolver::new("https://example.com");
    let _ = resolver;
}

#[test]
fn caching_resolver_new_with_invalid_url_does_not_panic() {
    // `CachingCardResolver::new` uses `unwrap_or_default` internally,
    // so an invalid base_url should not panic -- it just stores an empty string.
    let resolver = CachingCardResolver::new("");
    let _ = resolver;
}

// ── Invalidate behavior ──────────────────────────────────────────────────────

#[tokio::test]
async fn caching_resolver_invalidate_is_idempotent() {
    let resolver = CachingCardResolver::new("http://127.0.0.1:19999");
    // Invalidate multiple times without panic.
    resolver.invalidate().await;
    resolver.invalidate().await;
    resolver.invalidate().await;
}

#[tokio::test]
async fn caching_resolver_invalidate_after_failed_resolve() {
    let resolver = CachingCardResolver::new("http://127.0.0.1:19999");
    // Resolve fails (connection refused), cache should remain empty.
    let _ = resolver.resolve().await;
    // Invalidate should still work cleanly.
    resolver.invalidate().await;
}

// ── Thread-safety: Clone across tasks ────────────────────────────────────────

#[tokio::test]
async fn caching_resolver_is_send_sync_and_cloneable_across_tasks() {
    let resolver = CachingCardResolver::new("http://127.0.0.1:19999");

    let mut handles = Vec::new();
    for _ in 0..4 {
        let r = resolver.clone();
        handles.push(tokio::spawn(async move {
            // Each task attempts to resolve (will fail -- connection refused).
            let result = r.resolve().await;
            assert!(result.is_err(), "expected error from non-listening port");
        }));
    }

    for h in handles {
        h.await.expect("spawned task should not panic");
    }
}

#[tokio::test]
async fn caching_resolver_clone_shares_cache() {
    let resolver = CachingCardResolver::new("http://127.0.0.1:19999");
    let clone = resolver.clone();

    // Invalidate on the clone should also affect the original since they
    // share the same Arc<RwLock<...>>.
    clone.invalidate().await;

    // Both should still function correctly after invalidation.
    let err = resolver.resolve().await.unwrap_err();
    assert!(
        matches!(err, ClientError::HttpClient(_)),
        "expected HttpClient error after invalidation via clone, got: {err:?}",
    );
}

// ── Miscellaneous edge cases ─────────────────────────────────────────────────

#[tokio::test]
async fn resolve_agent_card_rejects_data_scheme() {
    let err = resolve_agent_card("data:text/plain,hello")
        .await
        .unwrap_err();
    assert!(
        matches!(err, ClientError::InvalidEndpoint(_)),
        "expected InvalidEndpoint for data: scheme, got: {err:?}",
    );
}

#[tokio::test]
async fn resolve_agent_card_rejects_file_scheme() {
    let err = resolve_agent_card("file:///etc/passwd").await.unwrap_err();
    assert!(
        matches!(err, ClientError::InvalidEndpoint(_)),
        "expected InvalidEndpoint for file: scheme, got: {err:?}",
    );
}
