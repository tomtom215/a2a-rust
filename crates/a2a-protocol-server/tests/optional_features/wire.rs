// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Optional behaviour seen on the wire, against one listener serving
//! JSON-RPC and HTTP+JSON:
//!
//! * §8.3.1: "URLs MAY be reused if multiple transports are available at
//!   the same endpoint": the card declares both bindings at one URL, and
//!   that URL serves both.
//! * §14: "The Agent Card MAY contain public information about an agent's
//!   capabilities": the card is served without credentials although every
//!   operation needs them.
//! * Appendix A.2: a server "MAY accept both legacy and current request
//!   message forms" and emits only the current form.

use std::sync::Arc;

use a2a_protocol_server::serve::Server;
use a2a_protocol_server::{
    BearerTokenAuthInterceptor, CombinedDispatcher, EventEmitter, RequestHandlerBuilder,
    agent_executor,
};
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::task::TaskState;
use serde_json::{Value, json};

use super::http;

struct Done;
agent_executor!(Done, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Completed)
        .await
});

const AUTH: (&str, &str) = ("authorization", "Bearer tok");

/// Both bindings on one listener, declared at one URL; every operation
/// needs the bearer token.
async fn server() -> (std::net::SocketAddr, String) {
    let server = Server::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr().unwrap();
    let url = format!("http://{addr}/");
    let card = AgentCard::new("both", "1.0.0", AgentInterface::jsonrpc(&url))
        .with_description("one URL, two bindings")
        .with_skill(AgentSkill::new("echo", "Echo", "echoes").with_tags(["echo"]))
        .with_capabilities(AgentCapabilities::none().with_streaming(true));
    let mut card = card;
    card.supported_interfaces.push(AgentInterface::rest(&url));
    let handler = RequestHandlerBuilder::new(Done)
        .with_agent_card(card)
        .with_interceptor(BearerTokenAuthInterceptor::new(["tok"]))
        .build()
        .unwrap();
    tokio::spawn(server.serve_with_shutdown(
        CombinedDispatcher::new(Arc::new(handler)),
        std::future::pending(),
    ));
    (addr, url)
}

fn jsonrpc_send(message: Value, configuration: Option<Value>) -> Value {
    let mut params = json!({ "message": message });
    if let Some(configuration) = configuration {
        params["configuration"] = configuration;
    }
    json!({"jsonrpc": "2.0", "id": 1, "method": "SendMessage", "params": params})
}

fn current_message(id: &str) -> Value {
    json!({"messageId": id, "role": "ROLE_USER", "parts": [{"text": "hi"}]})
}

#[tokio::test]
async fn two_bindings_declared_at_one_url_are_both_served_there() {
    let (addr, url) = server().await;

    let card = http(addr, "GET", "/.well-known/agent-card.json", None, &[]).await;
    assert_eq!(card.status, 200, "{}", card.body);
    let card: AgentCard = serde_json::from_value(card.body).unwrap();
    let declared: Vec<(&str, &str)> = card
        .supported_interfaces
        .iter()
        .map(|i| (i.protocol_binding.as_str(), i.url.as_str()))
        .collect();
    assert_eq!(
        declared,
        [("JSONRPC", url.as_str()), ("HTTP+JSON", url.as_str())]
    );

    // The JSON-RPC binding at the URL itself.
    let rpc = http(
        addr,
        "POST",
        "/",
        Some(jsonrpc_send(current_message("m-1"), None)),
        &[AUTH],
    )
    .await;
    assert_eq!(rpc.status, 200, "{}", rpc.body);
    assert_eq!(
        rpc.body["result"]["task"]["status"]["state"], "TASK_STATE_COMPLETED",
        "{}",
        rpc.body
    );

    // The HTTP+JSON binding under the same URL.
    let rest = http(
        addr,
        "POST",
        "/message:send",
        Some(json!({ "message": current_message("m-2") })),
        &[AUTH],
    )
    .await;
    assert_eq!(rest.status, 200, "{}", rest.body);
    assert_eq!(
        rest.body["task"]["status"]["state"], "TASK_STATE_COMPLETED",
        "{}",
        rest.body
    );
}

#[tokio::test]
async fn the_public_card_describes_capabilities_without_credentials() {
    let (addr, _) = server().await;

    // The control: an operation without the token is refused.
    let refused = http(addr, "GET", "/tasks/any", None, &[]).await;
    assert_eq!(refused.status, 401, "{}", refused.body);

    let card = http(addr, "GET", "/.well-known/agent-card.json", None, &[]).await;
    assert_eq!(card.status, 200, "{}", card.body);
    assert_eq!(card.body["skills"][0]["id"], "echo", "{}", card.body);
    assert_eq!(
        card.body["capabilities"]["streaming"], true,
        "{}",
        card.body
    );
}

#[tokio::test]
async fn a_legacy_request_form_is_accepted_and_answered_in_the_current_form() {
    let (addr, _) = server().await;

    // v0.3's lower-case role, and snake_case field names.
    let legacy_message =
        json!({"message_id": "m-legacy", "role": "user", "parts": [{"text": "hi"}]});
    let request = jsonrpc_send(legacy_message, Some(json!({"history_length": 1})));
    let reply = http(addr, "POST", "/", Some(request), &[AUTH]).await;
    assert_eq!(reply.status, 200, "{}", reply.body);

    let task = &reply.body["result"]["task"];
    assert_eq!(
        task["status"]["state"], "TASK_STATE_COMPLETED",
        "{}",
        reply.body
    );
    let echoed = &task["history"][0];
    assert_eq!(echoed["messageId"], "m-legacy", "{}", reply.body);
    assert_eq!(echoed["role"], "ROLE_USER", "{}", reply.body);
    assert!(echoed.get("message_id").is_none(), "{}", reply.body);
}
