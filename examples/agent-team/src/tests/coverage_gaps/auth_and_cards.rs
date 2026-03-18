// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests 67-70: Auth rejection, extended agent card, dynamic agent cards,
//! and agent card HTTP caching (ETag / 304 Not Modified).

use super::*;

// ── Real auth rejection (67) ────────────────────────────────────────────────

/// Interceptor that actually rejects requests without a valid bearer token.
struct RejectingAuthInterceptor {
    required_token: String,
}

impl a2a_protocol_server::interceptor::ServerInterceptor for RejectingAuthInterceptor {
    fn before<'a>(
        &'a self,
        ctx: &'a a2a_protocol_server::CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if ctx.caller_identity() != Some(&self.required_token) {
                Err(A2aError::new(
                    ErrorCode::UnsupportedOperation,
                    "auth rejected",
                ))
            } else {
                Ok(())
            }
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a a2a_protocol_server::CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

/// Test 67: A server interceptor that truly rejects unauthorized requests.
pub async fn test_real_auth_rejection(ctx: &TestContext) -> TestResult {
    let _ = ctx; // Uses its own ephemeral server.
    let start = Instant::now();

    // Spin up a dedicated agent with a rejecting interceptor.
    let (listener, addr) = bind_listener().await;
    let url = format!("http://{addr}");
    let card = AgentCard {
        url: None,
        name: "AuthTestAgent".into(),
        description: "Agent that rejects unauthenticated requests".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.clone(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "auth-test".into(),
            name: "Auth Test".into(),
            description: "Tests auth rejection".into(),
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
    };

    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
            .with_agent_card(card)
            .with_interceptor(RejectingAuthInterceptor {
                required_token: "super-secret".into(),
            })
            .build()
            .expect("build auth test handler"),
    );
    serve_jsonrpc(listener, handler);

    // Request without credentials -> should be rejected.
    let body = jsonrpc_request(serde_json::json!(1), "SendMessage", send_message_params());
    match post_raw(&url, &body).await {
        Ok((_status, resp_body)) => {
            let v: serde_json::Value = serde_json::from_str(&resp_body).unwrap_or_default();
            let is_error = v.get("error").is_some();
            let msg = v["error"]["message"].as_str().unwrap_or("");
            if is_error && msg.contains("rejected") {
                TestResult::pass(
                    "real-auth-rejection",
                    start.elapsed().as_millis(),
                    &format!("unauthenticated request rejected with message: {msg}"),
                )
            } else {
                TestResult::fail(
                    "real-auth-rejection",
                    start.elapsed().as_millis(),
                    &format!(
                        "expected error with 'rejected' message, got is_error={is_error}, msg={msg}, body={}",
                        &resp_body[..resp_body.len().min(100)]
                    ),
                )
            }
        }
        Err(e) => TestResult::fail("real-auth-rejection", start.elapsed().as_millis(), &e),
    }
}

// ── Extended agent card (68) ────────────────────────────────────────────────

/// Test 68: GetExtendedAgentCard via JSON-RPC returns the configured card.
pub async fn test_extended_agent_card(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let body = jsonrpc_request(
        serde_json::json!(1),
        "GetExtendedAgentCard",
        serde_json::json!({}),
    );
    match post_raw(&ctx.analyzer_url, &body).await {
        Ok((200, resp_body)) => {
            let v: serde_json::Value = serde_json::from_str(&resp_body).unwrap_or_default();
            let result = &v["result"];
            let name = result["name"].as_str().unwrap_or("");
            let has_skills = result
                .get("skills")
                .and_then(|s| s.as_array())
                .is_some_and(|a| !a.is_empty());
            if name == "Code Analyzer" && has_skills {
                TestResult::pass(
                    "extended-agent-card",
                    start.elapsed().as_millis(),
                    &format!("card name={name}, has_skills={has_skills}"),
                )
            } else {
                TestResult::fail(
                    "extended-agent-card",
                    start.elapsed().as_millis(),
                    &format!("expected name='Code Analyzer' with skills, got name={name}, has_skills={has_skills}"),
                )
            }
        }
        Ok((status, body)) => TestResult::fail(
            "extended-agent-card",
            start.elapsed().as_millis(),
            &format!("status={status}: {}", &body[..body.len().min(80)]),
        ),
        Err(e) => TestResult::fail("extended-agent-card", start.elapsed().as_millis(), &e),
    }
}

// ── Dynamic agent cards (69) ────────────────────────────────────────────────

/// A producer that returns a card with an incrementing counter.
struct CountingProducer {
    counter: std::sync::atomic::AtomicU32,
}

impl AgentCardProducer for CountingProducer {
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
        Box::pin(async move {
            let n = self
                .counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(AgentCard {
                url: None,
                name: format!("DynamicAgent-v{n}"),
                description: "Dynamically generated agent card".into(),
                version: format!("1.0.{n}"),
                supported_interfaces: vec![AgentInterface {
                    url: "http://localhost:0".into(),
                    protocol_binding: "JSONRPC".into(),
                    protocol_version: "1.0.0".into(),
                    tenant: None,
                }],
                default_input_modes: vec!["text/plain".into()],
                default_output_modes: vec!["text/plain".into()],
                skills: vec![],
                capabilities: AgentCapabilities::none(),
                provider: None,
                icon_url: None,
                documentation_url: None,
                security_schemes: None,
                security_requirements: None,
                signatures: None,
            })
        })
    }
}

/// Test 69: Dynamic agent card handler produces fresh cards on each request.
pub async fn test_dynamic_agent_card(_ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    let producer = CountingProducer {
        counter: std::sync::atomic::AtomicU32::new(0),
    };
    let handler = DynamicAgentCardHandler::new(producer);

    // First request — should get version 0.
    let req1 = Request::builder()
        .method("GET")
        .uri("/")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let resp1 = handler.handle(&req1).await;
    let body1 = resp1.into_body().collect().await.unwrap().to_bytes();
    let card1: serde_json::Value = serde_json::from_slice(&body1).unwrap_or_default();

    // Second request — should get version 1 (dynamic!).
    let req2 = Request::builder()
        .method("GET")
        .uri("/")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let resp2 = handler.handle(&req2).await;
    let body2 = resp2.into_body().collect().await.unwrap().to_bytes();
    let card2: serde_json::Value = serde_json::from_slice(&body2).unwrap_or_default();

    let name1 = card1["name"].as_str().unwrap_or("");
    let name2 = card2["name"].as_str().unwrap_or("");

    if name1 == "DynamicAgent-v0" && name2 == "DynamicAgent-v1" {
        TestResult::pass(
            "dynamic-agent-card",
            start.elapsed().as_millis(),
            &format!("{name1} -> {name2}"),
        )
    } else {
        TestResult::fail(
            "dynamic-agent-card",
            start.elapsed().as_millis(),
            &format!("expected DynamicAgent-v0 -> DynamicAgent-v1, got {name1} and {name2}"),
        )
    }
}

// ── Agent card HTTP caching (70) ────────────────────────────────────────────

/// Test 70: Agent card endpoint returns ETag; re-request with If-None-Match
/// yields 304 Not Modified.
pub async fn test_agent_card_caching(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let card_url = format!("{}/.well-known/agent.json", ctx.analyzer_url);

    // First request — get ETag.
    match get_raw(&card_url, &[]).await {
        Ok((200, headers, _body)) => {
            let etag = headers
                .iter()
                .find(|(k, _)| k == "etag")
                .map(|(_, v)| v.clone());
            let Some(etag) = etag else {
                return TestResult::fail(
                    "agent-card-caching",
                    start.elapsed().as_millis(),
                    "no ETag header in response",
                );
            };

            // Second request with If-None-Match — should get 304.
            match get_raw(&card_url, &[("if-none-match", &etag)]).await {
                Ok((304, _, _)) => TestResult::pass(
                    "agent-card-caching",
                    start.elapsed().as_millis(),
                    &format!("304 with ETag={etag}"),
                ),
                Ok((status, _, _)) => TestResult::fail(
                    "agent-card-caching",
                    start.elapsed().as_millis(),
                    &format!("expected 304 Not Modified, got {status}"),
                ),
                Err(e) => TestResult::fail("agent-card-caching", start.elapsed().as_millis(), &e),
            }
        }
        Ok((status, _, _)) => TestResult::fail(
            "agent-card-caching",
            start.elapsed().as_millis(),
            &format!("card fetch status={status}, expected 200"),
        ),
        Err(e) => TestResult::fail("agent-card-caching", start.elapsed().as_millis(), &e),
    }
}
