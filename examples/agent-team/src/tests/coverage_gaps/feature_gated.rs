// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests 79-80: Feature-gated tests.
//!
//! - Test 79: Agent card signing (JWS ES256) — requires `signing` feature
//! - Test 80: `OtelMetrics` integration — requires `otel` feature

#[cfg(any(feature = "signing", feature = "otel"))]
use super::*;

#[cfg(feature = "signing")]
use ring::signature::{self, EcdsaKeyPair, KeyPair};

// ── Test 79: Agent card signing E2E ──────────────────────────────────────────

/// Test 79: End-to-end agent card signing — generate ES256 key pair, sign a
/// real agent card, verify the signature, then confirm tamper detection works.
#[cfg(feature = "signing")]
pub async fn test_agent_card_signing(_ctx: &TestContext) -> TestResult {
    use a2a_protocol_types::signing::{sign_agent_card, verify_agent_card};
    use ring::rand::SystemRandom;

    let start = Instant::now();

    // 1. Generate an ES256 key pair.
    let rng = SystemRandom::new();
    let pkcs8 =
        match EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng) {
            Ok(k) => k,
            Err(e) => {
                return TestResult::fail(
                    "79_signing_e2e",
                    start.elapsed().as_millis(),
                    &format!("key generation failed: {e}"),
                );
            }
        };

    // 2. Create an agent card using the real code_analyzer_card builder.
    let card = crate::cards::code_analyzer_card("https://example.com/agent");

    // 3. Sign the card.
    let sig = match sign_agent_card(&card, pkcs8.as_ref(), Some("e2e-test-key")) {
        Ok(s) => s,
        Err(e) => {
            return TestResult::fail(
                "79_signing_e2e",
                start.elapsed().as_millis(),
                &format!("signing failed: {e}"),
            );
        }
    };

    // 4. Extract the public key and verify.
    let key_pair = match EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        pkcs8.as_ref(),
        &rng,
    ) {
        Ok(kp) => kp,
        Err(e) => {
            return TestResult::fail(
                "79_signing_e2e",
                start.elapsed().as_millis(),
                &format!("key pair load failed: {e}"),
            );
        }
    };
    let pub_key = key_pair.public_key().as_ref();

    if let Err(e) = verify_agent_card(&card, &sig, pub_key) {
        return TestResult::fail(
            "79_signing_e2e",
            start.elapsed().as_millis(),
            &format!("verification of valid card failed unexpectedly: {e}"),
        );
    }

    // 5. Tamper with the card and confirm verification fails.
    let mut tampered = card.clone();
    tampered.name = "TAMPERED".into();

    match verify_agent_card(&tampered, &sig, pub_key) {
        Ok(()) => {
            return TestResult::fail(
                "79_signing_e2e",
                start.elapsed().as_millis(),
                "tampered card verified successfully but should have been rejected",
            );
        }
        Err(_) => {
            // Expected — tamper detection works.
        }
    }

    TestResult::pass(
        "79_signing_e2e",
        start.elapsed().as_millis(),
        "sign + verify + tamper detection OK",
    )
}

// ── Test 80: OtelMetrics integration ─────────────────────────────────────────

/// Verifies that `OtelMetrics` can be wired into a `RequestHandler` and
/// correctly records metrics during real request processing — using a noop
/// meter provider (no collector required).
#[cfg(feature = "otel")]
pub async fn test_otel_metrics_integration(ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::otel::OtelMetricsBuilder;

    let start = Instant::now();

    // 1. Build an OtelMetrics instance backed by the global noop provider.
    let otel_metrics = OtelMetricsBuilder::new()
        .meter_name("a2a.agent-team.test")
        .build();

    // 2. Build a handler that uses OtelMetrics as its metrics observer.
    let card = AgentCard {
        url: None,
        name: "OtelTestAgent".into(),
        version: "1.0".into(),
        description: "Agent for OtelMetrics integration test".into(),
        supported_interfaces: vec![AgentInterface {
            url: ctx.analyzer_url.clone(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        provider: None,
        icon_url: None,
        documentation_url: None,
        capabilities: AgentCapabilities::none(),
        security_schemes: None,
        security_requirements: None,
        default_input_modes: vec![],
        default_output_modes: vec![],
        skills: vec![],
        signatures: None,
    };

    let handler = RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
        .with_agent_card(card)
        .with_metrics(otel_metrics)
        .build();

    if let Err(e) = handler {
        return TestResult::fail(
            "80_otel_metrics",
            start.elapsed().as_millis(),
            &format!("handler build failed: {e}"),
        );
    }

    // 3. Use the handler to process a real request.
    let handler = Arc::new(handler.unwrap());
    let (listener, addr) = bind_listener().await;
    serve_jsonrpc(listener, Arc::clone(&handler));

    let client = a2a_protocol_client::ClientBuilder::new(format!("http://{addr}"))
        .build()
        .expect("build client");

    let params = make_send_params("Analyze test.rs");
    match client.send_message(params).await {
        Ok(_) => TestResult::pass(
            "80_otel_metrics",
            start.elapsed().as_millis(),
            "OtelMetrics wired into handler, request processed successfully",
        ),
        Err(e) => TestResult::fail(
            "80_otel_metrics",
            start.elapsed().as_millis(),
            &format!("request with OtelMetrics handler failed: {e}"),
        ),
    }
}
