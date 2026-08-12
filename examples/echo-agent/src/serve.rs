// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Listener wiring for the four bindings.
//!
//! Every listener is pre-bound before the handler is built, so the agent card
//! can name the real address. An agent that listens on a port it never
//! announces is undiscoverable — §5 makes the card the only way a client
//! learns where to connect.
//!
//! The JSON-RPC and REST arms are near-identical by design. Their dispatchers
//! are different concrete types with their own `dispatch`, and a generic
//! wrapper over both costs more in trait plumbing than the ten duplicated
//! lines are worth in an example whose job is to be read.

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::handler::RequestHandler;

/// Binds an ephemeral port and returns the listener with its address.
pub async fn bind_listener() -> (tokio::net::TcpListener, SocketAddr) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local_addr");
    (listener, addr)
}

/// Starts the JSON-RPC binding (§9).
pub fn serve_jsonrpc(listener: tokio::net::TcpListener, handler: Arc<RequestHandler>) {
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    tokio::spawn(async move {
        loop {
            // A transient accept error must not end the loop: one refused
            // connection would silently drop a whole transport out of the
            // coverage matrix and read as a missing call.
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
}

/// Starts the HTTP+JSON binding (§11).
pub fn serve_rest(listener: tokio::net::TcpListener, handler: Arc<RequestHandler>) {
    let dispatcher = Arc::new(RestDispatcher::new(handler));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
}

/// Serves JSON-RPC *and* HTTP+JSON on one socket, routing by request shape.
///
/// The in-repo TCK points both its `--binding jsonrpc` and `--binding rest`
/// legs at a single `A2A_BIND_ADDR`, so both must answer there. Splitting them
/// onto separate ports silently breaks the JSON-RPC leg — which is what
/// happened while this module was being extracted, caught by re-reading the
/// TCK's own invocation rather than by any test.
pub async fn serve_combined(bind_addr: &str, handler: Arc<RequestHandler>) -> SocketAddr {
    let jsonrpc = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));
    let rest = Arc::new(RestDispatcher::new(handler));

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .unwrap_or_else(|e| panic!("bind {bind_addr}: {e}"));
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let jsonrpc = Arc::clone(&jsonrpc);
            let rest = Arc::clone(&rest);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                    let jsonrpc = Arc::clone(&jsonrpc);
                    let rest = Arc::clone(&rest);
                    async move {
                        // POST to the root is JSON-RPC; everything else — the
                        // `/v1/...` paths and `/.well-known/...` — is REST.
                        let is_jsonrpc = req.method() == hyper::Method::POST
                            && (req.uri().path() == "/" || req.uri().path().is_empty());
                        if is_jsonrpc {
                            Ok::<_, std::convert::Infallible>(jsonrpc.dispatch(req).await)
                        } else {
                            Ok::<_, std::convert::Infallible>(rest.dispatch(req).await)
                        }
                    }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });

    addr
}

/// Starts the gRPC binding (§10) and returns the bound address.
pub fn serve_grpc(listener: tokio::net::TcpListener, handler: Arc<RequestHandler>) -> SocketAddr {
    use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};

    GrpcDispatcher::new(handler, GrpcConfig::default())
        .serve_with_listener(listener)
        .expect("start gRPC server")
}

/// Starts the WebSocket binding (§12 custom) and returns the bound address.
///
/// The dispatcher owns its listener and performs the HTTP upgrade itself
/// rather than sharing the combined router, hence a separate port.
pub async fn serve_websocket(addr: &str, handler: Arc<RequestHandler>) -> SocketAddr {
    let ws = Arc::new(a2a_protocol_server::dispatch::websocket::WebSocketDispatcher::new(handler));
    ws.serve_with_addr(addr)
        .await
        .expect("bind websocket listener")
}
