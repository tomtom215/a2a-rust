// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The built-in tenant resolvers.

use super::*;

fn make_ctx() -> CallContext {
    CallContext::new("message/send")
}

// -- HeaderTenantResolver -------------------------------------------------

#[tokio::test]
async fn header_resolver_default_header() {
    let resolver = HeaderTenantResolver::default();
    let ctx = make_ctx().with_http_header("x-tenant-id", "acme");
    assert_eq!(resolver.resolve(&ctx).await, Some("acme".into()));
}

#[tokio::test]
async fn header_resolver_custom_header() {
    let resolver = HeaderTenantResolver::new("X-Org-Id");
    let ctx = make_ctx().with_http_header("x-org-id", "org-42");
    assert_eq!(resolver.resolve(&ctx).await, Some("org-42".into()));
}

#[tokio::test]
async fn header_resolver_missing_header() {
    let resolver = HeaderTenantResolver::default();
    let ctx = make_ctx();
    assert_eq!(resolver.resolve(&ctx).await, None);
}

// -- BearerTokenTenantResolver --------------------------------------------

/// Audit SEC-11. The default resolver names a token by its digest; the token
/// itself is in no tenant id, so it is in no task row, span or audit record.
#[tokio::test]
async fn bearer_resolver_names_the_token_by_its_digest_not_by_the_token() {
    let resolver = BearerTokenTenantResolver::default();
    let ctx = make_ctx().with_http_header("authorization", "Bearer tok_abc123");
    let tenant = resolver.resolve(&ctx).await.expect("a tenant");
    assert_eq!(tenant, "sha256:04859f28dd3b1f884b02c67417fd6d3b");
    assert!(!tenant.contains("tok_abc123"), "{tenant}");
    // The lowercase scheme is accepted, and names the same tenant.
    let lower = make_ctx().with_http_header("authorization", "bearer tok_abc123");
    assert_eq!(resolver.resolve(&lower).await, Some(tenant));
}

/// The deprecated constructor is the same resolver as the default.
#[tokio::test]
#[allow(deprecated)]
async fn the_deprecated_constructor_is_the_default() {
    let ctx = make_ctx().with_http_header("authorization", "Bearer tok_abc123");
    assert_eq!(
        BearerTokenTenantResolver::new().resolve(&ctx).await,
        BearerTokenTenantResolver::default().resolve(&ctx).await
    );
}

/// A mapper's answer is the tenant, as it returns it: only the default hashes.
#[tokio::test]
async fn a_mapper_is_not_hashed() {
    let resolver = BearerTokenTenantResolver::with_mapper(|token| Some(token.to_owned()));
    let ctx = make_ctx().with_http_header("authorization", "Bearer tok_abc123");
    assert_eq!(resolver.resolve(&ctx).await, Some("tok_abc123".into()));
}

/// Known answers from an independent SHA-256 (Python's `hashlib`), including
/// the three FIPS 180 vectors' leading bytes, a multi-byte token and a long one.
#[test]
fn the_tenant_id_is_the_first_16_bytes_of_the_sha_256_in_lowercase_hex() {
    for (token, expected) in [
        ("abc", "sha256:ba7816bf8f01cfea414140de5dae2223"),
        ("", "sha256:e3b0c44298fc1c149afbf4c8996fb924"),
        (
            "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "sha256:248d6a61d20638b8e5c026930c3e6039",
        ),
        ("é", "sha256:4a99557e4033c3539de2eb65472017ca"),
        ("日本語-token", "sha256:73b2af8e79f0477dff215f1e26f20ae7"),
    ] {
        assert_eq!(tenant_id_for_token(token), expected, "{token:?}");
    }
    let long = "a".repeat(1000);
    assert_eq!(
        tenant_id_for_token(&long),
        "sha256:41edece42d63e8d9bf515a9ba6932e1c"
    );
}

#[test]
fn a_tenant_id_has_one_shape_whatever_the_token() {
    let a = tenant_id_for_token("one");
    let b = tenant_id_for_token("two");
    assert_ne!(a, b, "different tokens, different tenants");
    assert_eq!(
        a,
        tenant_id_for_token("one"),
        "the same token, the same tenant"
    );
    for id in [&a, &b] {
        assert_eq!(id.len(), 39, "{id}");
        let hex = id.strip_prefix("sha256:").expect("the prefix");
        assert!(
            hex.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "lowercase hex only: {id}"
        );
    }
}

#[tokio::test]
async fn bearer_resolver_with_mapper() {
    let resolver = BearerTokenTenantResolver::with_mapper(|token| {
        token.strip_prefix("tok_").map(str::to_uppercase)
    });
    let ctx = make_ctx().with_http_header("authorization", "Bearer tok_abc");
    assert_eq!(resolver.resolve(&ctx).await, Some("ABC".into()));
}

#[tokio::test]
async fn bearer_resolver_mapper_returns_none() {
    let resolver = BearerTokenTenantResolver::with_mapper(|_| None);
    let ctx = make_ctx().with_http_header("authorization", "Bearer tok");
    assert_eq!(resolver.resolve(&ctx).await, None);
}

#[tokio::test]
async fn bearer_resolver_missing_header() {
    let resolver = BearerTokenTenantResolver::default();
    let ctx = make_ctx();
    assert_eq!(resolver.resolve(&ctx).await, None);
}

#[tokio::test]
async fn bearer_resolver_non_bearer_auth() {
    let resolver = BearerTokenTenantResolver::default();
    let ctx = make_ctx().with_http_header("authorization", "Basic abc123");
    assert_eq!(resolver.resolve(&ctx).await, None);
}

#[tokio::test]
async fn bearer_resolver_empty_token() {
    let resolver = BearerTokenTenantResolver::default();
    let ctx = make_ctx().with_http_header("authorization", "Bearer ");
    assert_eq!(resolver.resolve(&ctx).await, None);
}

// -- PathSegmentTenantResolver --------------------------------------------

#[tokio::test]
async fn path_resolver_extracts_segment() {
    let resolver = PathSegmentTenantResolver::new(1);
    let ctx = make_ctx().with_http_header(":path", "/tenants/acme/tasks");
    assert_eq!(resolver.resolve(&ctx).await, Some("acme".into()));
}

#[tokio::test]
async fn path_resolver_first_segment() {
    let resolver = PathSegmentTenantResolver::new(0);
    let ctx = make_ctx().with_http_header(":path", "/v1/agents");
    assert_eq!(resolver.resolve(&ctx).await, Some("v1".into()));
}

#[tokio::test]
async fn path_resolver_out_of_bounds() {
    let resolver = PathSegmentTenantResolver::new(10);
    let ctx = make_ctx().with_http_header(":path", "/a/b");
    assert_eq!(resolver.resolve(&ctx).await, None);
}

#[tokio::test]
async fn path_resolver_prefers_pseudo_header() {
    let resolver = PathSegmentTenantResolver::new(0);
    let ctx = make_ctx()
        .with_http_header(":path", "/h2-tenant/foo")
        .with_http_header("path", "/fallback/bar");
    assert_eq!(resolver.resolve(&ctx).await, Some("h2-tenant".into()));
}

#[tokio::test]
async fn path_resolver_missing_path() {
    let resolver = PathSegmentTenantResolver::new(0);
    let ctx = make_ctx();
    assert_eq!(resolver.resolve(&ctx).await, None);
}

/// `Default` is the resolver with no mapper: it names the token by its digest.
#[tokio::test]
async fn bearer_resolver_default_hashes_the_token() {
    let resolver = BearerTokenTenantResolver::default();
    let ctx = make_ctx().with_http_header("authorization", "Bearer test-token");
    assert_eq!(
        resolver.resolve(&ctx).await,
        Some(tenant_id_for_token("test-token")),
    );
}

/// Covers line 241 (`extract_from_path` with empty segment after filter).
#[tokio::test]
async fn path_resolver_uses_fallback_path_header() {
    let resolver = PathSegmentTenantResolver::new(0);
    // Only "path" header (no ":path") to test the fallback
    let ctx = make_ctx().with_http_header("path", "/tenant-from-path/tasks");
    assert_eq!(
        resolver.resolve(&ctx).await,
        None,
        "a client-settable `path` header must not select the tenant"
    );
}

/// The lowercase scheme is read like the usual one.
#[tokio::test]
async fn bearer_resolver_lowercase_bearer() {
    let resolver = BearerTokenTenantResolver::default();
    let ctx = make_ctx().with_http_header("authorization", "bearer lowercase_tok");
    assert_eq!(
        resolver.resolve(&ctx).await,
        Some(tenant_id_for_token("lowercase_tok"))
    );
}

#[test]
fn bearer_resolver_debug_shows_has_mapper() {
    let resolver = BearerTokenTenantResolver::default();
    let debug = format!("{resolver:?}");
    assert!(debug.contains("BearerTokenTenantResolver"));
    assert!(debug.contains("has_mapper"));
    assert!(debug.contains("false"));

    let resolver_with = BearerTokenTenantResolver::with_mapper(|t| Some(t.to_string()));
    let debug = format!("{resolver_with:?}");
    assert!(debug.contains("true"));
}
