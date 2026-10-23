// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Agent cards, from discovery and from `GetExtendedAgentCard`, held to the
//! rules a server publishes one by (`AgentCard::validate` and
//! `validate_security_schemes`).

use a2a_protocol_client::{ClientBuilder, ClientError};
use a2a_protocol_types::{AgentCard, Message, MessageRole, MessageSendParams, Part};

use super::assert_refused;
use super::peers::{self, Answer, Http};

/// A valid card whose one interface is `url`.
pub(super) fn card_json(url: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "a", "description": "d", "version": "1",
        "supportedInterfaces": [{"url": url, "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}],
        "provider": {"organization": "o", "url": "https://o.example"},
        "capabilities": {"extendedAgentCard": true},
        "defaultInputModes": ["text/plain"], "defaultOutputModes": ["text/plain"],
        "skills": [{"id": "s", "name": "S", "description": "d", "tags": ["t"]}],
        "signatures": [{"protected": "p", "signature": "s"}]
    })
}

type Break = (&'static str, fn(&mut serde_json::Value));

/// Among them the three the producer-side tests did not cover until
/// 2026-10-23 (idx 441, 445, 451), and the OAuth TLS rule.
const BROKEN_CARDS: [Break; 6] = [
    ("the agent provider needs an organization and a url", |c| {
        c["provider"]["url"] = "".into();
    }),
    ("every agent skill needs an id, name and description", |c| {
        c["skills"][0]["name"] = "".into();
    }),
    (
        "every card signature needs its protected header and signature",
        |c| {
            c["signatures"][0]["signature"] = "".into();
        },
    ),
    ("must declare at least one skill", |c| {
        c.as_object_mut().unwrap().remove("skills");
    }),
    (
        "every supported interface needs a url, protocolBinding and protocolVersion",
        |c| {
            c["supportedInterfaces"][0]["protocolVersion"] = "".into();
        },
    ),
    (
        "implicit flow's authorizationUrl and refreshUrl must be https",
        |c| {
            c["securitySchemes"] = serde_json::json!({"o": {"oauth2SecurityScheme": {"flows": {
            "implicit": {"authorizationUrl": "http://idp.example/authorize", "scopes": {}}}}}});
        },
    ),
];

/// A peer serving `card` at the well-known path (any GET), over HTTP/1.1.
async fn card_peer(card: serde_json::Value) -> peers::Peer {
    peers::http(
        Http::Rest,
        Answer {
            unary: card,
            ..Answer::default()
        },
    )
    .await
}

#[tokio::test]
async fn a_discovered_card_that_breaks_a_publishing_rule_is_refused() {
    let ok = card_peer(card_json("https://agent.example")).await;
    a2a_protocol_client::resolve_agent_card(&ok.url)
        .await
        .expect("the valid card");
    for (needle, break_it) in BROKEN_CARDS {
        let mut card = card_json("https://agent.example");
        break_it(&mut card);
        let bad = card_peer(card).await;
        let result = a2a_protocol_client::resolve_agent_card(&bad.url).await;
        assert_refused(result, "agent-card.json", needle);
    }
}

/// A refused extended card is not adopted: the capabilities it declares
/// are not acted on.
#[tokio::test]
async fn an_extended_card_that_breaks_a_publishing_rule_is_refused_and_not_adopted() {
    let params =
        || MessageSendParams::new(Message::new("m", MessageRole::User, vec![Part::text("hi")]));
    // Control: a valid extended card declaring streaming is adopted.
    let mut extended = card_json("https://agent.example");
    extended["capabilities"]["streaming"] = true.into();
    let peer = peers::http(
        Http::JsonRpc,
        Answer {
            unary: extended,
            ..Answer::default()
        },
    )
    .await;
    let public: AgentCard = serde_json::from_value(card_json(&peer.url)).expect("card");
    let client = ClientBuilder::from_card(&public)
        .expect("builder")
        .build()
        .expect("client");
    client
        .get_extended_agent_card()
        .await
        .expect("the valid card");
    assert!(
        !matches!(
            client.stream_message(params()).await,
            Err(ClientError::CapabilityNotDeclared { .. })
        ),
        "the valid extended card's streaming capability is adopted"
    );
    for (needle, break_it) in BROKEN_CARDS {
        let mut extended = card_json("https://agent.example");
        extended["capabilities"]["streaming"] = true.into();
        break_it(&mut extended);
        let peer = peers::http(
            Http::JsonRpc,
            Answer {
                unary: extended,
                ..Answer::default()
            },
        )
        .await;
        let public: AgentCard = serde_json::from_value(card_json(&peer.url)).expect("card");
        let client = ClientBuilder::from_card(&public)
            .expect("builder")
            .build()
            .expect("client");
        assert_refused(
            client.get_extended_agent_card().await,
            "GetExtendedAgentCard",
            needle,
        );
        assert!(
            matches!(
                client.stream_message(params()).await,
                Err(ClientError::CapabilityNotDeclared { .. })
            ),
            "{needle}: the refused card's streaming capability was adopted"
        );
    }
}
