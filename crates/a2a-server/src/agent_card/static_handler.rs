// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Static agent card handler.
//!
//! [`StaticAgentCardHandler`] serves a pre-serialized [`AgentCard`] as JSON.
//! The card is serialized once at construction time and served as raw bytes
//! on every request.

use a2a_types::agent_card::AgentCard;
use bytes::Bytes;
use http_body_util::Full;

use crate::agent_card::CORS_ALLOW_ALL;
use crate::error::ServerResult;

/// Serves a pre-serialized [`AgentCard`] as a JSON HTTP response.
#[derive(Debug, Clone)]
pub struct StaticAgentCardHandler {
    card_json: Bytes,
}

impl StaticAgentCardHandler {
    /// Creates a new handler by serializing the given [`AgentCard`] to JSON.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`](crate::error::ServerError) if serialization fails.
    pub fn new(card: &AgentCard) -> ServerResult<Self> {
        let json = serde_json::to_vec(card)?;
        Ok(Self {
            card_json: Bytes::from(json),
        })
    }

    /// Handles an agent card request, returning a `200 OK` JSON response.
    ///
    /// # Panics
    ///
    /// Panics if the response builder fails (should never happen).
    #[must_use]
    pub fn handle(&self) -> hyper::Response<Full<Bytes>> {
        hyper::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", CORS_ALLOW_ALL)
            .body(Full::new(self.card_json.clone()))
            .expect("response builder should not fail with valid headers")
    }
}
