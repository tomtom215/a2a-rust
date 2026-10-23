// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §13.3: "Extended cards MAY expose more detailed capability
//! information (e.g., rate limits, quotas)" and "MAY include
//! organization-specific or user-specific configuration". The first is
//! carried by the configured extended card, the second by an
//! `ExtendedCardProvider` choosing each caller's card.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_server::agent_card::ExtendedCardProvider;
use a2a_protocol_server::auth::BearerTokenAuthInterceptor;
use a2a_protocol_server::dispatch::RestDispatcher;
use a2a_protocol_server::error::ServerResult;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{CallContext, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::extensions::AgentExtension;
use a2a_protocol_types::security::{HttpAuthSecurityScheme, SecurityScheme};
use serde_json::json;

use super::{bearer, http};

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

const QUOTAS: &str = "https://example.com/ext/quotas/v1";
const WORKSPACE: &str = "https://example.com/ext/workspace/v1";

fn public() -> AgentCard {
    AgentCard::new(
        "quota",
        "1.0.0",
        AgentInterface::jsonrpc("http://localhost:1"),
    )
    .with_description("publishes its limits to authenticated callers")
    .with_skill(AgentSkill::new("chat", "Chat", "d").with_tags(["t"]))
    .with_capabilities(AgentCapabilities::none().with_extended_agent_card(true))
    // Spec §13.3: the scheme the bearer interceptor checks, in the public card.
    .with_security_schemes(std::collections::HashMap::from([(
        "bearer".to_owned(),
        SecurityScheme::Http(HttpAuthSecurityScheme {
            scheme: "Bearer".to_owned(),
            description: None,
            bearer_format: None,
        }),
    )]))
}

/// The public card, plus the limits that apply to a caller.
fn with_quotas() -> AgentCard {
    let mut card = public();
    card.capabilities.extensions = Some(vec![AgentExtension {
        uri: QUOTAS.into(),
        description: Some("rate limits and quotas".into()),
        required: Some(false),
        params: Some(json!({"requestsPerMinute": 600, "monthlyTasks": 10_000})),
    }]);
    card
}

/// The params of the extension `uri` on `card`, if it carries it.
fn params(card: &AgentCard, uri: &str) -> Option<serde_json::Value> {
    card.capabilities
        .extensions
        .iter()
        .flatten()
        .find(|e| e.uri == uri)
        .and_then(|e| e.params.clone())
}

#[tokio::test]
async fn extended_card_exposes_quota_capabilities_the_public_card_lacks() {
    let handler = RequestHandlerBuilder::new(Idle)
        .with_agent_card(public())
        .with_extended_agent_card(with_quotas())
        .with_interceptor(BearerTokenAuthInterceptor::new(["tok"]))
        .build()
        .unwrap();
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::new(handler)))
        .await
        .unwrap();

    let extended = http(
        addr,
        "GET",
        "/extendedAgentCard",
        None,
        &[("authorization", "Bearer tok")],
    )
    .await;
    assert_eq!(extended.status, 200, "{}", extended.body);
    let extended: AgentCard = serde_json::from_value(extended.body).unwrap();
    assert_eq!(
        params(&extended, QUOTAS),
        Some(json!({"requestsPerMinute": 600, "monthlyTasks": 10_000})),
        "{extended:?}"
    );

    let public = http(addr, "GET", "/.well-known/agent-card.json", None, &[]).await;
    assert_eq!(public.status, 200, "{}", public.body);
    let public: AgentCard = serde_json::from_value(public.body).unwrap();
    assert_eq!(params(&public, QUOTAS), None, "{public:?}");
}

/// Adds the caller's own workspace to the card it is served.
struct PerUser;
impl ExtendedCardProvider for PerUser {
    fn card_for<'a>(
        &'a self,
        ctx: &'a CallContext,
        configured: &'a AgentCard,
    ) -> Pin<Box<dyn Future<Output = ServerResult<AgentCard>> + Send + 'a>> {
        Box::pin(async move {
            let caller = ctx.caller_identity().unwrap_or_default().to_owned();
            let mut card = configured.clone();
            card.capabilities
                .extensions
                .get_or_insert_with(Vec::new)
                .push(AgentExtension {
                    uri: WORKSPACE.into(),
                    description: None,
                    required: Some(false),
                    params: Some(json!({ "workspaceOf": caller })),
                });
            Ok(card)
        })
    }
}

#[tokio::test]
async fn extended_card_carries_configuration_specific_to_the_caller() {
    let handler = RequestHandlerBuilder::new(Idle)
        .with_agent_card(public())
        .with_extended_agent_card(public())
        .with_extended_card_provider(PerUser)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
            ("a-tok", "alice"),
            ("b-tok", "bob"),
        ]))
        .build()
        .unwrap();

    let mut seen = Vec::new();
    for token in ["a-tok", "b-tok"] {
        let card = handler
            .on_get_extended_agent_card(Some(&bearer(token)))
            .await
            .unwrap();
        let workspace = params(&card, WORKSPACE).unwrap_or_else(|| panic!("{card:?}"));
        seen.push(workspace["workspaceOf"].as_str().unwrap().to_owned());
    }
    assert_ne!(seen[0], seen[1], "each caller gets its own: {seen:?}");
    assert!(
        seen[0].contains("alice") && seen[1].contains("bob"),
        "{seen:?}"
    );
}
