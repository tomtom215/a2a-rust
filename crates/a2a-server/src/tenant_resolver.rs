// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tenant resolution for multi-tenant A2A servers.
//!
//! [`TenantResolver`] extracts a tenant identifier from incoming requests,
//! enabling per-tenant routing, configuration, and resource isolation.
//!
//! # Built-in resolvers
//!
//! | Resolver | Strategy |
//! |---|---|
//! | [`HeaderTenantResolver`] | Reads a configurable HTTP header (default: `x-tenant-id`) |
//! | [`BearerTokenTenantResolver`] | Extracts `Authorization: Bearer <token>` and optionally maps it |
//! | [`PathSegmentTenantResolver`] | Extracts a URL path segment by index |
//!
//! # Example
//!
//! ```rust
//! use a2a_protocol_server::tenant_resolver::HeaderTenantResolver;
//! use a2a_protocol_server::CallContext;
//!
//! let resolver = HeaderTenantResolver::default();
//! let ctx = CallContext::new("message/send")
//!     .with_http_header("x-tenant-id", "acme-corp");
//!
//! // resolver.resolve(&ctx) would return Some("acme-corp".into())
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::call_context::CallContext;

// ── Trait ────────────────────────────────────────────────────────────────────

/// Trait for extracting a tenant identifier from incoming requests.
///
/// Implement this to customize how tenant identity is determined — e.g. from
/// HTTP headers, JWT claims, URL path segments, or API keys.
///
/// # Object safety
///
/// This trait is designed to be used behind `Arc<dyn TenantResolver>`.
///
/// # Return value
///
/// `None` means no tenant could be determined; the server should use its
/// default partition / configuration.
pub trait TenantResolver: Send + Sync + 'static {
    /// Extracts the tenant identifier from the given call context.
    ///
    /// Returns `None` if no tenant can be determined (uses default partition).
    fn resolve<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>>;
}

// ── HeaderTenantResolver ─────────────────────────────────────────────────────

/// Extracts a tenant ID from a configurable HTTP header.
///
/// By default reads `x-tenant-id`. The header name is always matched
/// case-insensitively (keys in [`CallContext::http_headers`] are lowercased).
///
/// # Example
///
/// ```rust
/// use a2a_protocol_server::tenant_resolver::HeaderTenantResolver;
///
/// // Default: reads "x-tenant-id"
/// let resolver = HeaderTenantResolver::default();
///
/// // Custom header:
/// let resolver = HeaderTenantResolver::new("x-org-id");
/// ```
#[derive(Debug, Clone)]
pub struct HeaderTenantResolver {
    header_name: String,
}

impl HeaderTenantResolver {
    /// Creates a new resolver that reads the given HTTP header.
    ///
    /// The `header_name` is lowercased automatically.
    #[must_use]
    pub fn new(header_name: impl Into<String>) -> Self {
        Self {
            header_name: header_name.into().to_ascii_lowercase(),
        }
    }
}

impl Default for HeaderTenantResolver {
    fn default() -> Self {
        Self::new("x-tenant-id")
    }
}

impl TenantResolver for HeaderTenantResolver {
    fn resolve<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>> {
        Box::pin(async move { ctx.http_headers.get(&self.header_name).cloned() })
    }
}

// ── BearerTokenTenantResolver ────────────────────────────────────────────────

/// Type alias for the optional mapping function applied to the bearer token.
type TokenMapper = Arc<dyn Fn(&str) -> Option<String> + Send + Sync + 'static>;

/// Extracts a tenant ID from the `Authorization: Bearer <token>` header.
///
/// By default, uses the raw bearer token as the tenant identifier. An optional
/// mapping function can transform or validate the token (e.g. decode a JWT
/// and extract a `tenant_id` claim).
///
/// # Example
///
/// ```rust
/// use a2a_protocol_server::tenant_resolver::BearerTokenTenantResolver;
///
/// // Use the raw token as tenant ID:
/// let resolver = BearerTokenTenantResolver::new();
///
/// // With a custom mapping:
/// let resolver = BearerTokenTenantResolver::with_mapper(|token| {
///     // e.g. decode JWT, look up tenant in cache, etc.
///     Some(format!("tenant-for-{token}"))
/// });
/// ```
pub struct BearerTokenTenantResolver {
    mapper: Option<TokenMapper>,
}

impl std::fmt::Debug for BearerTokenTenantResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BearerTokenTenantResolver")
            .field("has_mapper", &self.mapper.is_some())
            .finish()
    }
}

impl BearerTokenTenantResolver {
    /// Creates a resolver that uses the raw bearer token as the tenant ID.
    #[must_use]
    pub fn new() -> Self {
        Self { mapper: None }
    }

    /// Creates a resolver with a custom mapping function.
    ///
    /// The mapper receives the bearer token (without the `Bearer ` prefix) and
    /// returns an optional tenant ID. Return `None` to indicate that the token
    /// does not map to a valid tenant.
    #[must_use]
    pub fn with_mapper<F>(mapper: F) -> Self
    where
        F: Fn(&str) -> Option<String> + Send + Sync + 'static,
    {
        Self {
            mapper: Some(Arc::new(mapper)),
        }
    }
}

impl Default for BearerTokenTenantResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl TenantResolver for BearerTokenTenantResolver {
    fn resolve<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>> {
        Box::pin(async move {
            let auth = ctx.http_headers.get("authorization")?;
            let token = auth
                .strip_prefix("Bearer ")
                .or_else(|| auth.strip_prefix("bearer "))?;

            if token.is_empty() {
                return None;
            }

            self.mapper
                .as_ref()
                .map_or_else(|| Some(token.to_owned()), |mapper| mapper(token))
        })
    }
}

// ── PathSegmentTenantResolver ────────────────────────────────────────────────

/// Extracts a tenant ID from a URL path segment by index.
///
/// Path segments are split by `/`, with empty segments (from leading `/`)
/// removed. For example, the path `/tenants/acme/tasks` has segments
/// `["tenants", "acme", "tasks"]`; index `1` yields `"acme"`.
///
/// The resolver reads the path from the `:path` pseudo-header (HTTP/2) or
/// the lowercased `path` key in [`CallContext::http_headers`]. If neither is
/// present, resolution returns `None`.
///
/// # Example
///
/// ```rust
/// use a2a_protocol_server::tenant_resolver::PathSegmentTenantResolver;
///
/// // Extract segment at index 1: /tenants/{id}/...
/// let resolver = PathSegmentTenantResolver::new(1);
/// ```
#[derive(Debug, Clone)]
pub struct PathSegmentTenantResolver {
    segment_index: usize,
}

impl PathSegmentTenantResolver {
    /// Creates a resolver that extracts the path segment at the given index.
    ///
    /// Index `0` is the first non-empty segment after the leading `/`.
    #[must_use]
    pub const fn new(segment_index: usize) -> Self {
        Self { segment_index }
    }

    /// Extracts the tenant ID from a raw path string.
    fn extract_from_path(&self, path: &str) -> Option<String> {
        let segment = path
            .split('/')
            .filter(|s| !s.is_empty())
            .nth(self.segment_index)?;

        if segment.is_empty() {
            None
        } else {
            Some(segment.to_owned())
        }
    }
}

impl TenantResolver for PathSegmentTenantResolver {
    fn resolve<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>> {
        Box::pin(async move {
            // Try :path pseudo-header first (HTTP/2), then "path".
            let path = ctx
                .http_headers
                .get(":path")
                .or_else(|| ctx.http_headers.get("path"))?;
            self.extract_from_path(path)
        })
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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

    #[tokio::test]
    async fn bearer_resolver_raw_token() {
        let resolver = BearerTokenTenantResolver::new();
        let ctx = make_ctx().with_http_header("authorization", "Bearer tok_abc123");
        assert_eq!(resolver.resolve(&ctx).await, Some("tok_abc123".into()));
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
        let resolver = BearerTokenTenantResolver::new();
        let ctx = make_ctx();
        assert_eq!(resolver.resolve(&ctx).await, None);
    }

    #[tokio::test]
    async fn bearer_resolver_non_bearer_auth() {
        let resolver = BearerTokenTenantResolver::new();
        let ctx = make_ctx().with_http_header("authorization", "Basic abc123");
        assert_eq!(resolver.resolve(&ctx).await, None);
    }

    #[tokio::test]
    async fn bearer_resolver_empty_token() {
        let resolver = BearerTokenTenantResolver::new();
        let ctx = make_ctx().with_http_header("authorization", "Bearer ");
        assert_eq!(resolver.resolve(&ctx).await, None);
    }

    // -- PathSegmentTenantResolver --------------------------------------------

    #[tokio::test]
    async fn path_resolver_extracts_segment() {
        let resolver = PathSegmentTenantResolver::new(1);
        let ctx = make_ctx().with_http_header("path", "/tenants/acme/tasks");
        assert_eq!(resolver.resolve(&ctx).await, Some("acme".into()));
    }

    #[tokio::test]
    async fn path_resolver_first_segment() {
        let resolver = PathSegmentTenantResolver::new(0);
        let ctx = make_ctx().with_http_header("path", "/v1/agents");
        assert_eq!(resolver.resolve(&ctx).await, Some("v1".into()));
    }

    #[tokio::test]
    async fn path_resolver_out_of_bounds() {
        let resolver = PathSegmentTenantResolver::new(10);
        let ctx = make_ctx().with_http_header("path", "/a/b");
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

    /// Covers lines 172-174 (`BearerTokenTenantResolver` Default impl).
    #[tokio::test]
    async fn bearer_resolver_default_same_as_new() {
        let resolver = BearerTokenTenantResolver::default();
        let ctx = make_ctx().with_http_header("authorization", "Bearer test-token");
        assert_eq!(
            resolver.resolve(&ctx).await,
            Some("test-token".into()),
            "default() should behave the same as new()"
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
            Some("tenant-from-path".into())
        );
    }

    /// Covers lowercase bearer prefix variant (line 186).
    #[tokio::test]
    async fn bearer_resolver_lowercase_bearer() {
        let resolver = BearerTokenTenantResolver::new();
        let ctx = make_ctx().with_http_header("authorization", "bearer lowercase_tok");
        assert_eq!(resolver.resolve(&ctx).await, Some("lowercase_tok".into()));
    }

    #[test]
    fn bearer_resolver_debug_shows_has_mapper() {
        let resolver = BearerTokenTenantResolver::new();
        let debug = format!("{resolver:?}");
        assert!(debug.contains("BearerTokenTenantResolver"));
        assert!(debug.contains("has_mapper"));
        assert!(debug.contains("false"));

        let resolver_with = BearerTokenTenantResolver::with_mapper(|t| Some(t.to_string()));
        let debug = format!("{resolver_with:?}");
        assert!(debug.contains("true"));
    }
}
