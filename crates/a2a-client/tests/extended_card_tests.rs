// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Integration test: `A2aClient::get_extended_agent_card()` against a real
//! JSON-RPC server.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::error::A2aResult;

// ── Noop executor (never called) ────────────────────────────────────────────

struct NoopExecutor;

impl AgentExecutor for NoopExecutor {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn test_agent_card() -> AgentCard {
    AgentCard {
        url: None,
        name: "Extended Test Agent".into(),
        description: "Agent for extended card test".into(),
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
            id: "private-skill".into(),
            name: "Private Skill".into(),
            description: "A private skill visible only via extended card".into(),
            tags: vec!["private".into()],
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_extended_agent_card_returns_card() {
    // Build a real JSON-RPC server with an agent card configured.
    let handler = Arc::new(
        RequestHandlerBuilder::new(NoopExecutor)
            .with_agent_card(test_agent_card())
            .build()
            .expect("build handler"),
    );
    let dispatcher = JsonRpcDispatcher::new(handler);

    let addr = serve_with_addr("127.0.0.1:0", dispatcher)
        .await
        .expect("serve_with_addr should bind");

    // Build a client pointing at the server.
    let base_url = format!("http://{addr}");
    let client = ClientBuilder::new(&base_url).build().expect("build client");

    // Call the method under test.
    let card = client
        .get_extended_agent_card()
        .await
        .expect("get_extended_agent_card should succeed");

    assert_eq!(card.name, "Extended Test Agent");
    assert_eq!(card.skills.len(), 1);
    assert_eq!(card.skills[0].id, "private-skill");
    assert_eq!(card.version, "1.0.0");
}
