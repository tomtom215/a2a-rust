// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Agent card discovery.
//!
//! A2A agents publish their [`AgentCard`] at a well-known URL. This module
//! provides helpers to fetch and parse the card.
//!
//! The default discovery path is `/.well-known/agent.json` appended to
//! the agent's base URL.

use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use a2a_types::AgentCard;

use crate::error::{ClientError, ClientResult};

/// The standard well-known path for agent card discovery.
pub const AGENT_CARD_PATH: &str = "/.well-known/agent.json";

// ── Public API ────────────────────────────────────────────────────────────────

/// Fetches the [`AgentCard`] from the standard well-known path.
///
/// Appends `/.well-known/agent-card.json` to `base_url` and performs an
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
    let url = build_card_url(base_url, AGENT_CARD_PATH)?;
    fetch_card(&url).await
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
    fetch_card(&url).await
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
    fetch_card(url).await
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

async fn fetch_card(url: &str) -> ClientResult<AgentCard> {
    let client: Client<HttpConnector, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();

    let req = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(url)
        .header(header::ACCEPT, "application/json")
        .body(Full::new(Bytes::new()))
        .map_err(|e| ClientError::Transport(e.to_string()))?;

    let resp = tokio::time::timeout(Duration::from_secs(30), client.request(req))
        .await
        .map_err(|_| ClientError::Transport("agent card fetch timed out".into()))?
        .map_err(|e| ClientError::HttpClient(e.to_string()))?;

    let status = resp.status();
    let body_bytes = resp.collect().await.map_err(ClientError::Http)?.to_bytes();

    if !status.is_success() {
        let body_str = String::from_utf8_lossy(&body_bytes).into_owned();
        return Err(ClientError::UnexpectedStatus {
            status: status.as_u16(),
            body: body_str,
        });
    }

    serde_json::from_slice::<AgentCard>(&body_bytes).map_err(ClientError::Serialization)
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
}
