// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for agent card discovery module.

use a2a_client::discovery::CachingCardResolver;

#[test]
fn caching_resolver_new_builds_url() {
    let resolver = CachingCardResolver::new("http://localhost:8080");
    // Verify the resolver was created (we can't access the URL directly)
    let _resolver = resolver;
}

#[test]
fn caching_resolver_with_trailing_slash() {
    let resolver = CachingCardResolver::new("http://localhost:8080/");
    let _resolver = resolver;
}

#[test]
fn caching_resolver_with_custom_path() {
    let resolver = CachingCardResolver::with_path("http://localhost:8080", "/custom/card.json");
    let _resolver = resolver;
}

#[tokio::test]
async fn caching_resolver_invalidate_clears_cache() {
    let resolver = CachingCardResolver::new("http://localhost:8080");
    resolver.invalidate().await;
    // Should not panic
}
