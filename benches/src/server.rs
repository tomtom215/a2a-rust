// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Server startup helpers for benchmarks.
//!
//! Provides functions to spin up an in-process A2A server on an ephemeral port
//! so that transport benchmarks can hit a real HTTP endpoint without any
//! external dependencies.
//!
//! ## Socket reuse
//!
//! All servers set `SO_REUSEADDR` on the listener socket so that rapid
//! server creation/destruction cycles (e.g. cold-start benchmarks) don't
//! fail with `AddrInUse` when a previous socket is still in `TIME_WAIT`.
//! This is critical for CI environments where kernel `TIME_WAIT` recycling
//! is slower than on developer machines.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use tokio::sync::watch;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::Dispatcher;

use crate::fixtures;

// ── Server handle ───────────────────────────────────────────────────────────

/// A running benchmark server with its bound address and shutdown handle.
///
/// When dropped, the server's accept loop is signalled to stop and all
/// in-flight connections are allowed to drain. This prevents socket leak
/// and `AddrInUse` errors during rapid server cycling (cold-start benchmarks).
pub struct BenchServer {
    pub addr: SocketAddr,
    pub url: String,
    /// Dropping this sender signals the accept loop to shut down.
    _shutdown: watch::Sender<bool>,
}

// ── Startup helpers ─────────────────────────────────────────────────────────

/// Starts a JSON-RPC server on an ephemeral port with the given executor.
pub async fn start_jsonrpc_server(executor: impl AgentExecutor) -> BenchServer {
    let listener = bind_reusable_listener().await;
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{addr}");

    let handler = Arc::new(
        RequestHandlerBuilder::new(executor)
            .with_agent_card(fixtures::agent_card(&url))
            .build()
            .expect("build benchmark handler"),
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    let shutdown = spawn_hyper_server(listener, dispatcher).await;
    BenchServer {
        addr,
        url,
        _shutdown: shutdown,
    }
}

/// Starts a JSON-RPC server with push notification support enabled.
///
/// Required for benchmarks that exercise push config CRUD operations
/// (set/get/list/delete push notification configs). Uses a [`crate::executor::NoopPushSender`]
/// that accepts all webhook URLs without performing actual HTTP delivery.
pub async fn start_jsonrpc_server_with_push(executor: impl AgentExecutor) -> BenchServer {
    use crate::executor::NoopPushSender;

    let listener = bind_reusable_listener().await;
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
    let shutdown = spawn_hyper_server(listener, dispatcher).await;
    BenchServer {
        addr,
        url,
        _shutdown: shutdown,
    }
}

/// Starts a REST server on an ephemeral port with the given executor.
pub async fn start_rest_server(executor: impl AgentExecutor) -> BenchServer {
    let listener = bind_reusable_listener().await;
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{addr}");

    let handler = Arc::new(
        RequestHandlerBuilder::new(executor)
            .with_agent_card(fixtures::agent_card(&url))
            .build()
            .expect("build benchmark handler"),
    );
    let dispatcher = Arc::new(RestDispatcher::new(handler));
    let shutdown = spawn_hyper_server(listener, dispatcher).await;
    BenchServer {
        addr,
        url,
        _shutdown: shutdown,
    }
}

// ── Internal helpers ────────────────────────────────────────────────────────

/// Binds a TCP listener on an ephemeral port with `SO_REUSEADDR` enabled.
///
/// `SO_REUSEADDR` allows binding to an address that is still in `TIME_WAIT`
/// state from a recently closed socket. This is essential for the cold-start
/// benchmark, which creates and destroys servers faster than the kernel
/// recycles sockets — especially on CI runners.
async fn bind_reusable_listener() -> tokio::net::TcpListener {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .expect("create socket");
    socket.set_reuse_address(true).expect("set SO_REUSEADDR");
    // SO_REUSEPORT allows multiple sockets to bind to the same address on
    // Linux. This further reduces the chance of AddrInUse under rapid cycling.
    #[cfg(target_os = "linux")]
    socket.set_reuse_port(true).expect("set SO_REUSEPORT");
    socket.set_nonblocking(true).expect("set nonblocking");
    socket
        .bind(
            &"127.0.0.1:0"
                .parse::<std::net::SocketAddr>()
                .unwrap()
                .into(),
        )
        .expect("bind benchmark server");
    socket.listen(128).expect("listen on benchmark server");
    tokio::net::TcpListener::from_std(socket.into()).expect("convert to tokio TcpListener")
}

/// Spawns the hyper accept loop and returns a shutdown sender.
///
/// When the returned `watch::Sender<bool>` is dropped, the accept loop
/// exits gracefully. In-flight connections continue to completion but
/// no new connections are accepted.
async fn spawn_hyper_server(
    listener: tokio::net::TcpListener,
    dispatcher: Arc<dyn Dispatcher>,
) -> watch::Sender<bool> {
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;

                // Check shutdown signal first to ensure prompt termination.
                _ = shutdown_rx.changed() => break,

                result = listener.accept() => {
                    let Ok((stream, _)) = result else { continue };
                    // Disable Nagle's algorithm — matches production serve.rs.
                    let _ = stream.set_nodelay(true);
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let dispatcher = Arc::clone(&dispatcher);
                    tokio::spawn(async move {
                        let svc = hyper::service::service_fn(
                            move |req: hyper::Request<Incoming>| {
                                let d = Arc::clone(&dispatcher);
                                async move {
                                    Ok::<hyper::Response<BoxBody<Bytes, Infallible>>, Infallible>(
                                        d.dispatch(req).await,
                                    )
                                }
                            },
                        );
                        let _ = hyper_util::server::conn::auto::Builder::new(
                            hyper_util::rt::TokioExecutor::new(),
                        )
                        .serve_connection(io, svc)
                        .await;
                    });
                }
            }
        }
    });

    shutdown_tx
}
