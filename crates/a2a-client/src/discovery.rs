// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Agent card discovery with HTTP caching.
//!
//! A2A agents publish their [`AgentCard`] at a well-known URL. This module
//! provides helpers to fetch and parse the card.
//!
//! The default discovery path is `/.well-known/agent.json` appended to
//! the agent's base URL.
//!
//! Per spec §8.3, the client supports HTTP caching via `ETag` and
//! `If-None-Match` / `If-Modified-Since` conditional request headers.

use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header;
#[cfg(not(feature = "tls-rustls"))]
use hyper_util::client::legacy::connect::HttpConnector;
#[cfg(not(feature = "tls-rustls"))]
use hyper_util::client::legacy::Client;
#[cfg(not(feature = "tls-rustls"))]
use hyper_util::rt::TokioExecutor;
use tokio::sync::RwLock;

use a2a_protocol_types::AgentCard;

use crate::error::{ClientError, ClientResult};

/// The standard well-known path for agent card discovery.
pub const AGENT_CARD_PATH: &str = "/.well-known/agent.json";

// ── Public API ────────────────────────────────────────────────────────────────

/// Fetches the [`AgentCard`] from the standard well-known path.
///
/// Appends `/.well-known/agent.json` to `base_url` and performs an
/// HTTP GET.
///
/// # Errors
///
/// - [`ClientError::InvalidEndpoint`] — `base_url` is malformed.
/// - [`ClientError::HttpClient`] — connection error.
/// - [`ClientError::UnexpectedStatus`] — server returned a non-200 status.
/// - [`ClientError::Serialization`] — response body is not a valid
///   [`AgentCard`].
pub async fn resolve_agent_card(base_url: &str) -> ClientResult<AgentCard> {
    trace_info!(base_url, "resolving agent card");
    let url = build_card_url(base_url, AGENT_CARD_PATH)?;
    fetch_card(&url, None).await
}

/// Fetches the [`AgentCard`] from a custom path.
///
/// Unlike [`resolve_agent_card`], this function appends `path` (not the
/// standard well-known path) to `base_url`.
///
/// # Errors
///
/// Same conditions as [`resolve_agent_card`].
pub async fn resolve_agent_card_with_path(base_url: &str, path: &str) -> ClientResult<AgentCard> {
    let url = build_card_url(base_url, path)?;
    fetch_card(&url, None).await
}

/// Fetches the [`AgentCard`] from an absolute URL.
///
/// The URL must be a complete `http://` or `https://` URL pointing directly
/// at the agent card JSON resource.
///
/// # Errors
///
/// Same conditions as [`resolve_agent_card`].
pub async fn fetch_card_from_url(url: &str) -> ClientResult<AgentCard> {
    fetch_card(url, None).await
}

// ── Cached Discovery ─────────────────────────────────────────────────────────

/// Cached entry for an agent card, holding the card and its `ETag`.
#[derive(Debug, Clone)]
struct CachedCard {
    card: AgentCard,
    etag: Option<String>,
    last_modified: Option<String>,
}

/// A caching agent card resolver.
///
/// Stores the last fetched card and uses conditional HTTP requests
/// (`If-None-Match`, `If-Modified-Since`) to avoid unnecessary re-downloads
/// (spec §8.3).
#[derive(Debug, Clone)]
pub struct CachingCardResolver {
    url: String,
    cache: Arc<RwLock<Option<CachedCard>>>,
}

impl CachingCardResolver {
    /// Creates a new resolver for the given agent card URL.
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        let url = build_card_url(base_url, AGENT_CARD_PATH).unwrap_or_default();
        Self {
            url,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Creates a new resolver with a custom path.
    #[must_use]
    pub fn with_path(base_url: &str, path: &str) -> Self {
        let url = build_card_url(base_url, path).unwrap_or_default();
        Self {
            url,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Resolves the agent card, using a cached version if valid.
    ///
    /// Sends conditional request headers when a cached card exists. On `304`,
    /// returns the cached card. On `200`, updates the cache and returns the
    /// new card.
    ///
    /// # Errors
    ///
    /// Same conditions as [`resolve_agent_card`].
    pub async fn resolve(&self) -> ClientResult<AgentCard> {
        trace_info!(url = %self.url, "resolving agent card (cached)");
        let cached = self.cache.read().await.clone();
        let (card, etag, last_modified) =
            fetch_card_with_metadata(&self.url, cached.as_ref()).await?;

        // Update cache with new metadata.
        {
            let mut guard = self.cache.write().await;
            *guard = Some(CachedCard {
                card: card.clone(),
                etag,
                last_modified,
            });
        }

        Ok(card)
    }

    /// Clears the internal cache.
    pub async fn invalidate(&self) {
        let mut cache = self.cache.write().await;
        *cache = None;
    }
}

// ── internals ─────────────────────────────────────────────────────────────────

fn build_card_url(base_url: &str, path: &str) -> ClientResult<String> {
    if base_url.is_empty() {
        return Err(ClientError::InvalidEndpoint(
            "base URL must not be empty".into(),
        ));
    }
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err(ClientError::InvalidEndpoint(format!(
            "base URL must start with http:// or https://: {base_url}"
        )));
    }
    let base = base_url.trim_end_matches('/');
    let path = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    Ok(format!("{base}{path}"))
}

async fn fetch_card(url: &str, cached: Option<&CachedCard>) -> ClientResult<AgentCard> {
    let (card, _, _) = fetch_card_with_metadata(url, cached).await?;
    Ok(card)
}

async fn fetch_card_with_metadata(
    url: &str,
    cached: Option<&CachedCard>,
) -> ClientResult<(AgentCard, Option<String>, Option<String>)> {
    #[cfg(not(feature = "tls-rustls"))]
    let client: Client<HttpConnector, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();

    #[cfg(feature = "tls-rustls")]
    let client = crate::tls::build_https_client();

    let mut builder = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(url)
        .header(header::ACCEPT, "application/json");

    // Add conditional request headers if we have cached data.
    if let Some(cached) = cached {
        if let Some(ref etag) = cached.etag {
            builder = builder.header("if-none-match", etag.as_str());
        }
        if let Some(ref lm) = cached.last_modified {
            builder = builder.header("if-modified-since", lm.as_str());
        }
    }

    let req = builder
        .body(Full::new(Bytes::new()))
        .map_err(|e| ClientError::Transport(e.to_string()))?;

    let resp = tokio::time::timeout(Duration::from_secs(30), client.request(req))
        .await
        .map_err(|_| ClientError::Transport("agent card fetch timed out".into()))?
        .map_err(|e| ClientError::HttpClient(e.to_string()))?;

    let status = resp.status();

    // 304 Not Modified — return cached card with existing metadata.
    if status == hyper::StatusCode::NOT_MODIFIED {
        if let Some(cached) = cached {
            return Ok((
                cached.card.clone(),
                cached.etag.clone(),
                cached.last_modified.clone(),
            ));
        }
        // No cached card but got 304 — shouldn't happen, fall through to error.
    }

    // Extract caching headers before consuming the response body.
    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let last_modified = resp
        .headers()
        .get("last-modified")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let body_bytes = resp.collect().await.map_err(ClientError::Http)?.to_bytes();

    if !status.is_success() {
        let body_str = String::from_utf8_lossy(&body_bytes).into_owned();
        return Err(ClientError::UnexpectedStatus {
            status: status.as_u16(),
            body: body_str,
        });
    }

    let card =
        serde_json::from_slice::<AgentCard>(&body_bytes).map_err(ClientError::Serialization)?;
    Ok((card, etag, last_modified))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_card_url_standard() {
        let url = build_card_url("http://localhost:8080", AGENT_CARD_PATH).unwrap();
        assert_eq!(url, "http://localhost:8080/.well-known/agent.json");
    }

    #[test]
    fn build_card_url_trailing_slash() {
        let url = build_card_url("http://localhost:8080/", AGENT_CARD_PATH).unwrap();
        assert_eq!(url, "http://localhost:8080/.well-known/agent.json");
    }

    #[test]
    fn build_card_url_custom_path() {
        let url = build_card_url("http://localhost:8080", "/api/card.json").unwrap();
        assert_eq!(url, "http://localhost:8080/api/card.json");
    }

    #[test]
    fn build_card_url_rejects_empty() {
        assert!(build_card_url("", AGENT_CARD_PATH).is_err());
    }

    #[test]
    fn build_card_url_rejects_non_http() {
        assert!(build_card_url("ftp://example.com", AGENT_CARD_PATH).is_err());
    }

    #[test]
    fn caching_resolver_new() {
        let resolver = CachingCardResolver::new("http://localhost:8080");
        assert_eq!(resolver.url, "http://localhost:8080/.well-known/agent.json");
    }

    #[test]
    fn caching_resolver_with_path() {
        let resolver = CachingCardResolver::with_path("http://localhost:8080", "/custom/card.json");
        assert_eq!(resolver.url, "http://localhost:8080/custom/card.json");
    }

    #[tokio::test]
    async fn caching_resolver_invalidate() {
        let resolver = CachingCardResolver::new("http://localhost:8080");
        // Cache should start empty.
        assert!(resolver.cache.read().await.is_none());
        resolver.invalidate().await;
        assert!(resolver.cache.read().await.is_none());
    }
}
