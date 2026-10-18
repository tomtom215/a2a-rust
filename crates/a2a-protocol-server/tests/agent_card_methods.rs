// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The agent card's path answers only `GET` and `HEAD` (spec §8.2), on
//! every listener: any other method is `405 Method Not Allowed` with
//! `Allow: GET, HEAD` (RFC 9110 §15.5.6: the server "MUST generate an
//! `Allow` header field").
//!
//! Neither a conditional header nor a missing `A2A-Version` changes that:
//! the card is discovered before a client knows the version, so its path is
//! versionless for every method.
//!
//! A conditional header does not change it either. RFC 9110 §13.1.2 answers a
//! matching `If-None-Match` with `412` for a method other than `GET` or
//! `HEAD` — but only for a method the resource performs: §13.2.1, "a server
//! MUST ignore all received preconditions if its response to the same
//! request without those conditions ... would have been a status code other
//! than a 2xx (Successful) or 412". The card performs no other method, so
//! its `405` stands, and `412` is never the answer.

#![cfg(all(feature = "websocket", feature = "grpc", feature = "axum"))]

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_server::A2aRouter;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher, WebSocketDispatcher};
use a2a_protocol_server::handler::RequestHandler;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{EventEmitter, agent_executor};
use a2a_protocol_types::TaskState;
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PATH: &str = "/.well-known/agent-card.json";

struct Done;
agent_executor!(Done, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Completed)
        .await
});

fn handler() -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Done)
            .with_agent_card(
                AgentCard::new(
                    "methods",
                    "1.0.0",
                    AgentInterface::jsonrpc("http://localhost:1"),
                )
                .with_description("d")
                .with_skill(AgentSkill::new("s", "S", "d").with_tags(["t"])),
            )
            .build()
            .unwrap(),
    )
}

/// The status and the `Allow` header of one HTTP/1.1 request.
async fn ask(addr: SocketAddr, method: &str, extra: &str) -> (u16, Option<String>) {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(
        format!(
            "{method} {PATH} HTTP/1.1\r\nHost: localhost\r\n\
             Content-Length: 0\r\nConnection: close\r\n{extra}\r\n"
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut out = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), s.read_to_end(&mut out))
        .await
        .expect("answered and closed")
        .unwrap();
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let allow = text
        .split("\r\n\r\n")
        .next()
        .unwrap_or("")
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("allow")
                .then(|| value.trim().to_owned())
        });
    (status, allow)
}

async fn listeners() -> Vec<(&'static str, SocketAddr)> {
    let jsonrpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler()))
        .await
        .unwrap();
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler()))
        .await
        .unwrap();
    let axum_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let axum = axum_listener.local_addr().unwrap();
    let app = A2aRouter::new(handler()).into_router();
    tokio::spawn(async move { axum::serve(axum_listener, app).await });
    let websocket = Arc::new(WebSocketDispatcher::new(handler()))
        .serve_with_addr("127.0.0.1:0")
        .await
        .unwrap();
    let grpc = GrpcDispatcher::new(handler(), GrpcConfig::default())
        .serve_with_addr("127.0.0.1:0")
        .await
        .unwrap();
    vec![
        ("JSON-RPC", jsonrpc),
        ("REST", rest),
        ("axum", axum),
        ("WebSocket", websocket),
        ("gRPC", grpc),
    ]
}

#[tokio::test]
async fn every_other_method_is_405_with_allow_and_never_412() {
    let mut wrong = Vec::new();
    for (name, addr) in listeners().await {
        for method in ["POST", "PUT", "DELETE", "PATCH"] {
            for extra in [
                "A2A-Version: 1.0\r\n",
                "A2A-Version: 1.0\r\nIf-None-Match: *\r\n",
                "If-None-Match: *\r\n",
                "",
            ] {
                let (status, allow) = ask(addr, method, extra).await;
                let allowed: Vec<String> = allow
                    .iter()
                    .flat_map(|a| a.split(','))
                    .map(|m| m.trim().to_ascii_uppercase())
                    .collect();
                let ok = status == 405
                    && allowed.iter().any(|m| m == "GET")
                    && allowed.iter().any(|m| m == "HEAD");
                if !ok {
                    wrong.push(format!(
                        "{name} {method} [{}]: {status}, Allow {allow:?}",
                        extra.trim_end().replace("\r\n", "; ")
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The conditional `GET` is unchanged: a matching `If-None-Match` is `304`.
#[tokio::test]
async fn a_conditional_get_is_still_304() {
    for (name, addr) in listeners().await {
        let (status, _) = ask(addr, "GET", "If-None-Match: *\r\n").await;
        assert_eq!(status, 304, "{name}");
    }
}
