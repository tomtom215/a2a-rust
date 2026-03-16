// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Integration tests for `DynamicAgentCardHandler`.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::error::{A2aError, A2aResult};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};

use a2a_protocol_server::agent_card::dynamic_handler::{AgentCardProducer, DynamicAgentCardHandler};

/// Minimal agent card for tests.
fn test_card() -> AgentCard {
    AgentCard {
        name: "Dynamic Agent".into(),
        description: "A dynamically produced card".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "https://agent.example.com/rpc".into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "test".into(),
            name: "Test".into(),
            description: "Test skill".into(),
            tags: vec![],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// A simple producer that always returns the same card.
struct StaticProducer(AgentCard);

impl AgentCardProducer for StaticProducer {
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
        Box::pin(async { Ok(self.0.clone()) })
    }
}

/// A producer that tracks call count.
struct CountingProducer {
    card: AgentCard,
    count: AtomicU32,
}

impl AgentCardProducer for CountingProducer {
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
        self.count.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Ok(self.card.clone()) })
    }
}

/// A producer that always returns an error.
struct ErrorProducer;

impl AgentCardProducer for ErrorProducer {
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
        Box::pin(async { Err(A2aError::internal("producer failed")) })
    }
}

fn make_request() -> hyper::Request<Full<Bytes>> {
    hyper::Request::builder()
        .body(Full::new(Bytes::new()))
        .unwrap()
}

fn make_request_with_header(name: &str, value: &str) -> hyper::Request<Full<Bytes>> {
    hyper::Request::builder()
        .header(name, value)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

// ── Basic functionality ─────────────────────────────────────────────────────

#[tokio::test]
async fn handle_returns_200_with_json_content_type() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));
    let req = make_request();
    let resp = handler.handle(&req).await;

    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json"
    );
}

#[tokio::test]
async fn handle_returns_valid_agent_card_json() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));
    let req = make_request();
    let resp = handler.handle(&req).await;

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let card: AgentCard = serde_json::from_slice(&body).expect("response should be valid JSON");
    assert_eq!(card.name, "Dynamic Agent");
    assert_eq!(card.version, "1.0.0");
}

#[tokio::test]
async fn handle_includes_etag_header() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));
    let req = make_request();
    let resp = handler.handle(&req).await;

    let etag = resp
        .headers()
        .get("etag")
        .expect("ETag header should be present");
    let etag_str = etag.to_str().unwrap();
    assert!(
        etag_str.starts_with("W/\""),
        "ETag should be weak: {etag_str}"
    );
}

#[tokio::test]
async fn handle_includes_last_modified_header() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));
    let req = make_request();
    let resp = handler.handle(&req).await;

    let lm = resp
        .headers()
        .get("last-modified")
        .expect("Last-Modified should be present");
    let lm_str = lm.to_str().unwrap();
    assert!(
        lm_str.ends_with("GMT"),
        "Last-Modified should end with GMT: {lm_str}"
    );
}

#[tokio::test]
async fn handle_includes_cache_control_header() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));
    let req = make_request();
    let resp = handler.handle(&req).await;

    let cc = resp
        .headers()
        .get("cache-control")
        .expect("Cache-Control should be present");
    assert!(cc.to_str().unwrap().contains("max-age="));
}

#[tokio::test]
async fn handle_includes_cors_header() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));
    let req = make_request();
    let resp = handler.handle(&req).await;

    let cors = resp
        .headers()
        .get("access-control-allow-origin")
        .expect("CORS header should be present");
    assert_eq!(cors.to_str().unwrap(), "*");
}

// ── Custom cache max-age ────────────────────────────────────────────────────

#[tokio::test]
async fn custom_max_age_is_reflected() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card())).with_max_age(120);
    let req = make_request();
    let resp = handler.handle(&req).await;

    let cc = resp
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        cc.contains("max-age=120"),
        "Expected max-age=120, got: {cc}"
    );
}

// ── Conditional requests (If-None-Match) ────────────────────────────────────

#[tokio::test]
async fn if_none_match_matching_etag_returns_304() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));

    // First request to get the ETag.
    let resp = handler.handle(&make_request()).await;
    let etag = resp
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // Second request with matching If-None-Match.
    let req = make_request_with_header("if-none-match", &etag);
    let resp2 = handler.handle(&req).await;
    assert_eq!(resp2.status(), 304);
}

#[tokio::test]
async fn if_none_match_non_matching_returns_200() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));

    let req = make_request_with_header("if-none-match", "W/\"definitely-wrong\"");
    let resp = handler.handle(&req).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn if_none_match_wildcard_returns_304() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));

    let req = make_request_with_header("if-none-match", "*");
    let resp = handler.handle(&req).await;
    assert_eq!(resp.status(), 304);
}

// ── Conditional requests (If-Modified-Since) ────────────────────────────────

#[tokio::test]
async fn if_modified_since_matching_returns_304() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));

    // First request to get the Last-Modified value.
    let resp = handler.handle(&make_request()).await;
    let lm = resp
        .headers()
        .get("last-modified")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // Second request with matching If-Modified-Since.
    let req = make_request_with_header("if-modified-since", &lm);
    let resp2 = handler.handle(&req).await;
    assert_eq!(resp2.status(), 304);
}

#[tokio::test]
async fn if_modified_since_non_matching_returns_200() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));

    let req = make_request_with_header("if-modified-since", "Thu, 01 Jan 1970 00:00:00 GMT");
    let resp = handler.handle(&req).await;
    assert_eq!(resp.status(), 200);
}

// ── Producer error handling ─────────────────────────────────────────────────

#[tokio::test]
async fn producer_error_returns_500() {
    let handler = DynamicAgentCardHandler::new(ErrorProducer);
    let req = make_request();
    let resp = handler.handle(&req).await;
    assert_eq!(resp.status(), 500);
}

#[tokio::test]
async fn producer_error_returns_json_error_body() {
    let handler = DynamicAgentCardHandler::new(ErrorProducer);
    let req = make_request();
    let resp = handler.handle(&req).await;

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).expect("should be JSON");
    assert!(
        v.get("error").is_some(),
        "error body should contain 'error' key"
    );
}

// ── handle_unconditional ────────────────────────────────────────────────────

#[tokio::test]
async fn handle_unconditional_returns_200() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));
    let resp = handler.handle_unconditional().await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn handle_unconditional_includes_all_headers() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));
    let resp = handler.handle_unconditional().await;

    assert!(resp.headers().get("etag").is_some());
    assert!(resp.headers().get("last-modified").is_some());
    assert!(resp.headers().get("cache-control").is_some());
    assert!(resp.headers().get("content-type").is_some());
    assert!(resp.headers().get("access-control-allow-origin").is_some());
}

#[tokio::test]
async fn handle_unconditional_error_returns_500() {
    let handler = DynamicAgentCardHandler::new(ErrorProducer);
    let resp = handler.handle_unconditional().await;
    assert_eq!(resp.status(), 500);
}

// ── Producer invocation ─────────────────────────────────────────────────────

#[tokio::test]
async fn producer_is_called_on_every_request() {
    let producer = CountingProducer {
        card: test_card(),
        count: AtomicU32::new(0),
    };
    // Need to use Arc for shared ownership
    let producer = Arc::new(producer);
    let handler = DynamicAgentCardHandler::new(ArcProducer(Arc::clone(&producer)));

    handler.handle(&make_request()).await;
    handler.handle(&make_request()).await;
    handler.handle(&make_request()).await;

    assert_eq!(producer.count.load(Ordering::Relaxed), 3);
}

/// Wrapper to use Arc<CountingProducer> as AgentCardProducer.
struct ArcProducer(Arc<CountingProducer>);

impl AgentCardProducer for ArcProducer {
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
        self.0.produce()
    }
}

// ── ETag determinism ────────────────────────────────────────────────────────

#[tokio::test]
async fn same_card_produces_same_etag() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));

    let resp1 = handler.handle(&make_request()).await;
    let etag1 = resp1
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    let resp2 = handler.handle(&make_request()).await;
    let etag2 = resp2
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    assert_eq!(etag1, etag2, "Same card data should produce same ETag");
}

#[tokio::test]
async fn different_cards_produce_different_etags() {
    let card1 = test_card();
    let mut card2 = test_card();
    card2.name = "Different Agent".into();

    let h1 = DynamicAgentCardHandler::new(StaticProducer(card1));
    let h2 = DynamicAgentCardHandler::new(StaticProducer(card2));

    let etag1 = h1
        .handle(&make_request())
        .await
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let etag2 = h2
        .handle(&make_request())
        .await
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    assert_ne!(
        etag1, etag2,
        "Different card data should produce different ETags"
    );
}

// ── 304 response body is empty ──────────────────────────────────────────────

#[tokio::test]
async fn not_modified_response_has_empty_body() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));

    let resp = handler.handle(&make_request()).await;
    let etag = resp
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    let req = make_request_with_header("if-none-match", &etag);
    let resp2 = handler.handle(&req).await;
    assert_eq!(resp2.status(), 304);
    let body = resp2.into_body().collect().await.unwrap().to_bytes();
    assert!(body.is_empty(), "304 response should have empty body");
}

// ── If-None-Match takes precedence over If-Modified-Since (RFC 7232 §6) ─────

#[tokio::test]
async fn if_none_match_takes_precedence_over_if_modified_since() {
    let handler = DynamicAgentCardHandler::new(StaticProducer(test_card()));

    let resp = handler.handle(&make_request()).await;
    let etag = resp
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // Matching ETag but non-matching If-Modified-Since — should still return 304.
    let req = hyper::Request::builder()
        .header("if-none-match", &etag)
        .header("if-modified-since", "Thu, 01 Jan 1970 00:00:00 GMT")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let resp2 = handler.handle(&req).await;
    assert_eq!(resp2.status(), 304, "If-None-Match should take precedence");
}
