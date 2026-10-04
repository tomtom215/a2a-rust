// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Standing agents up: each is its own `RequestHandler` on its own loopback
//! port — separate stores, separate queues, separate connections — so the
//! swarm's traffic is real HTTP between real A2A servers, not calls inside one.

use std::sync::Arc;

use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::{
    AgentExecutor, JsonRpcDispatcher, RequestHandlerBuilder, serve_with_addr,
};
use a2a_protocol_types::{AgentCard, AgentInterface};

/// Binds an ephemeral port, serves `executor` there over JSON-RPC, and
/// returns the base URL. The card has to carry the port actually bound, and
/// the handler is built before the listener, so the port is learned first.
pub async fn spawn_agent(name: &str, executor: impl AgentExecutor + 'static) -> String {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("probe bind");
    let addr = probe.local_addr().expect("local_addr");
    drop(probe);
    let url = format!("http://{addr}");
    let card = AgentCard::new(name, "0.0.0", AgentInterface::jsonrpc(&url)).with_streaming(true);
    let handler = Arc::new(
        RequestHandlerBuilder::new(executor)
            .with_agent_card(card)
            .build()
            .expect("static handler config"),
    );
    serve_with_addr(addr, JsonRpcDispatcher::new(handler))
        .await
        .expect("bind agent port");
    url
}

pub fn client(url: &str) -> A2aClient {
    ClientBuilder::new(url)
        .build()
        .expect("client for a loopback URL")
}
