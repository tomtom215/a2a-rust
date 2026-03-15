// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Dynamic agent card handler.
//!
//! [`DynamicAgentCardHandler`] calls an [`AgentCardProducer`] on every request
//! to generate a fresh [`AgentCard`]. This is useful when the card contents
//! depend on runtime state (e.g. feature flags, authenticated context).

use std::future::Future;
use std::pin::Pin;

use a2a_types::agent_card::AgentCard;
use a2a_types::error::A2aResult;
use bytes::Bytes;
use http_body_util::Full;

use crate::agent_card::CORS_ALLOW_ALL;

/// Trait for producing an [`AgentCard`] dynamically.
///
/// Object-safe; used behind `Arc<dyn AgentCardProducer>` or as a generic bound.
pub trait AgentCardProducer: Send + Sync + 'static {
    /// Produces an [`AgentCard`] for the current request.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if card generation fails.
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>>;
}

/// Serves a dynamically generated [`AgentCard`] as a JSON HTTP response.
#[derive(Debug)]
pub struct DynamicAgentCardHandler<P> {
    producer: P,
}

impl<P: AgentCardProducer> DynamicAgentCardHandler<P> {
    /// Creates a new handler with the given producer.
    #[must_use]
    pub const fn new(producer: P) -> Self {
        Self { producer }
    }

    /// Handles an agent card request by producing a fresh card and serializing it.
    ///
    /// # Panics
    ///
    /// Panics if the response builder fails (should never happen).
    pub async fn handle(&self) -> hyper::Response<Full<Bytes>> {
        match self.producer.produce().await {
            Ok(card) => match serde_json::to_vec(&card) {
                Ok(json) => hyper::Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", CORS_ALLOW_ALL)
                    .body(Full::new(Bytes::from(json)))
                    .expect("response builder should not fail with valid headers"),
                Err(e) => error_response(500, &format!("serialization error: {e}")),
            },
            Err(e) => error_response(500, &format!("card producer error: {e}")),
        }
    }
}

/// Builds a simple JSON error response.
fn error_response(status: u16, message: &str) -> hyper::Response<Full<Bytes>> {
    let body = serde_json::json!({ "error": message });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    hyper::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(bytes)))
        .expect("response builder should not fail with valid headers")
}
