// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Audit B-25, ADR 0055: JSON-RPC, HTTP+JSON and WebSocket on one port, a
//! `CombinedDispatcher` with `with_websocket`: this SDK's `WebSocketTransport`
//! is upgraded on the HTTP listener and its calls reach the same tasks as
//! the HTTP bindings', and the card that declares all three is read from
//! that listener. The upgrade's refusals and shutdown are
//! `a2a-protocol-server/tests/websocket_on_http_port.rs`.

#![cfg(feature = "websocket")]

use std::sync::Arc;

use a2a_protocol_client::transport::websocket::WebSocketTransport;
use a2a_protocol_client::{ClientBuilder, resolve_agent_card};
use a2a_protocol_server::dispatch::{CombinedDispatcher, WebSocketDispatcher};
use a2a_protocol_server::serve::Server;
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::Message;
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;

struct Completes;
agent_executor!(Completes, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Completed)
        .await
});

fn task_of(sent: SendMessageResponse) -> a2a_protocol_types::task::Task {
    match sent {
        SendMessageResponse::Task(task) => task,
        other => panic!("not a task: {other:?}"),
    }
}

#[tokio::test]
async fn one_port_serves_jsonrpc_rest_and_websocket() {
    let server = Server::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr().unwrap();
    let (http, ws) = (format!("http://{addr}"), format!("ws://{addr}/"));
    let card = AgentCard::new("one-port", "1.0.0", AgentInterface::jsonrpc(http.clone()))
        .with_interface(AgentInterface::rest(http.clone()))
        .with_interface(AgentInterface::new(
            ws.clone(),
            a2a_protocol_types::WEBSOCKET_BINDING_URI,
        ))
        .with_description("d")
        .with_skill(AgentSkill::new("s", "S", "d").with_tags(["t"]));
    let handler = Arc::new(
        RequestHandlerBuilder::new(Completes)
            .with_agent_card(card)
            .build()
            .unwrap(),
    );
    let dispatcher = CombinedDispatcher::new(Arc::clone(&handler))
        .with_websocket(WebSocketDispatcher::new(handler));
    tokio::spawn(server.serve_with_shutdown(dispatcher, std::future::pending()));

    let websocket = ClientBuilder::new(&ws)
        .with_custom_transport(WebSocketTransport::connect(&ws).await.unwrap())
        .build()
        .unwrap();
    let jsonrpc = ClientBuilder::new(&http)
        .with_protocol_binding("JSONRPC")
        .build()
        .unwrap();
    let rest = ClientBuilder::new(&http)
        .with_protocol_binding("HTTP+JSON")
        .build()
        .unwrap();

    let over_ws = task_of(
        websocket
            .send_message(MessageSendParams::new(Message::user_text("w-1", "hi")))
            .await
            .unwrap(),
    );
    assert_eq!(over_ws.status.state, TaskState::Completed);
    for client in [&jsonrpc, &rest] {
        let read = client
            .get_task(TaskQueryParams::new(over_ws.id.0.clone()))
            .await
            .unwrap();
        assert_eq!(
            read.id, over_ws.id,
            "the HTTP bindings see the WebSocket's task"
        );
    }

    let over_http = task_of(
        rest.send_message(MessageSendParams::new(Message::user_text("r-1", "hi")))
            .await
            .unwrap(),
    );
    let read = websocket
        .get_task(TaskQueryParams::new(over_http.id.0.clone()))
        .await
        .unwrap();
    assert_eq!(read.id, over_http.id, "and the WebSocket sees theirs");

    let card = resolve_agent_card(&http).await.unwrap();
    assert!(
        card.supported_interfaces
            .iter()
            .any(|i| i.url == ws && i.protocol_binding == a2a_protocol_types::WEBSOCKET_BINDING_URI),
        "{card:?}"
    );
}
