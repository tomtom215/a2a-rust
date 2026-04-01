// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Server startup helpers for benchmarks.
//!
//! Provides functions to spin up an in-process A2A server on an ephemeral port
//! so that transport benchmarks can hit a real HTTP endpoint without any
//! external dependencies.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::Dispatcher;

use crate::fixtures;

// ── Server handle ───────────────────────────────────────────────────────────

/// A running benchmark server with its bound address.
pub struct BenchServer {
    pub addr: SocketAddr,
    pub url: String,
}

// ── Startup helpers ─────────────────────────────────────────────────────────

/// Starts a JSON-RPC server on an ephemeral port with the given executor.
pub async fn start_jsonrpc_server(executor: impl AgentExecutor) -> BenchServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind benchmark server");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{addr}");

    let handler = Arc::new(
        RequestHandlerBuilder::new(executor)
            .with_agent_card(fixtures::agent_card(&url))
            .build()
            .expect("build benchmark handler"),
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    spawn_hyper_server(listener, dispatcher).await;
    BenchServer { addr, url }
}

/// Starts a JSON-RPC server with push notification support enabled.
///
/// Required for benchmarks that exercise push config CRUD operations
/// (set/get/list/delete push notification configs). Uses a [`crate::executor::NoopPushSender`]
/// that accepts all webhook URLs without performing actual HTTP delivery.
pub async fn start_jsonrpc_server_with_push(executor: impl AgentExecutor) -> BenchServer {
    use crate::executor::NoopPushSender;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind benchmark server");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{addr}");

    let mut card = fixtures::agent_card(&url);
    card.capabilities = card.capabilities.with_push_notifications(true);

    let handler = Arc::new(
        RequestHandlerBuilder::new(executor)
            .with_agent_card(card)
            .with_push_sender(NoopPushSender)
            .build()
            .expect("build benchmark handler with push"),
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    spawn_hyper_server(listener, dispatcher).await;
    BenchServer { addr, url }
}

/// Starts a REST server on an ephemeral port with the given executor.
pub async fn start_rest_server(executor: impl AgentExecutor) -> BenchServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind benchmark server");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{addr}");

    let handler = Arc::new(
        RequestHandlerBuilder::new(executor)
            .with_agent_card(fixtures::agent_card(&url))
            .build()
            .expect("build benchmark handler"),
    );
    let dispatcher = Arc::new(RestDispatcher::new(handler));
    spawn_hyper_server(listener, dispatcher).await;
    BenchServer { addr, url }
}

// ── Internal helpers ────────────────────────────────────────────────────────

async fn spawn_hyper_server(listener: tokio::net::TcpListener, dispatcher: Arc<dyn Dispatcher>) {
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            // Disable Nagle's algorithm — matches production serve.rs.
            let _ = stream.set_nodelay(true);
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                    let d = Arc::clone(&dispatcher);
                    async move {
                        Ok::<hyper::Response<BoxBody<Bytes, Infallible>>, Infallible>(
                            d.dispatch(req).await,
                        )
                    }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, svc)
                .await;
            });
        }
    });
}
