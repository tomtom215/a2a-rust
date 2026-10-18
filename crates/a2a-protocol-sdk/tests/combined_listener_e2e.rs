// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! JSON-RPC and HTTP+JSON on one listener, through `serve_with_addr` and a
//! `CombinedDispatcher`, with no hand-written accept loop: a client of each
//! binding works against the one URL, both see the same tasks, and a client
//! that resolves the card reads it from that listener.

use std::sync::Arc;

use a2a_protocol_client::{ClientBuilder, resolve_agent_card};
use a2a_protocol_server::dispatch::CombinedDispatcher;
use a2a_protocol_server::serve::serve_with_addr;
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

async fn listener(jsonrpc_path: &str) -> String {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let url = format!("http://{addr}");
    let card = AgentCard::new(
        "combined",
        "1.0.0",
        AgentInterface::jsonrpc(format!("{url}{jsonrpc_path}")),
    )
    .with_interface(AgentInterface::rest(url.clone()))
    .with_description("d")
    .with_skill(AgentSkill::new("s", "S", "d").with_tags(["t"]));
    let handler = Arc::new(
        RequestHandlerBuilder::new(Completes)
            .with_agent_card(card)
            .build()
            .unwrap(),
    );
    let dispatcher = CombinedDispatcher::new(handler)
        .with_jsonrpc_path(jsonrpc_path)
        .unwrap();
    serve_with_addr(addr, dispatcher).await.unwrap();
    url
}

#[tokio::test]
async fn both_bindings_answer_on_one_listener_and_share_tasks() {
    for path in ["/", "/a2a"] {
        let url = listener(path).await;
        let jsonrpc = ClientBuilder::new(format!("{url}{path}"))
            .with_protocol_binding("JSONRPC")
            .build()
            .unwrap();
        let rest = ClientBuilder::new(url.clone())
            .with_protocol_binding("HTTP+JSON")
            .build()
            .unwrap();

        let sent = jsonrpc
            .send_message(MessageSendParams::new(Message::user_text("m-1", "hi")))
            .await
            .unwrap();
        let SendMessageResponse::Task(task) = sent else {
            panic!("JSON-RPC send: {sent:?}");
        };
        let read = rest
            .get_task(TaskQueryParams::new(task.id.0.clone()))
            .await
            .unwrap();
        assert_eq!(read.id, task.id, "{path}");
        assert_eq!(read.status.state, TaskState::Completed, "{path}");

        let sent = rest
            .send_message(MessageSendParams::new(Message::user_text("m-2", "hi")))
            .await
            .unwrap();
        let SendMessageResponse::Task(task) = sent else {
            panic!("REST send: {sent:?}");
        };
        let read = jsonrpc
            .get_task(TaskQueryParams::new(task.id.0.clone()))
            .await
            .unwrap();
        assert_eq!(read.id, task.id, "{path}");

        let card = resolve_agent_card(&url).await.unwrap();
        assert_eq!(card.name, "combined", "{path}");
    }
}
