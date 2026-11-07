// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The three listeners the gate serves one handler on, each on a free
//! loopback port for the rest of the test.

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_sdk::server::{
    Dispatcher, GrpcConfig, GrpcDispatcher, JsonRpcDispatcher, RequestHandler, RestDispatcher,
    Server,
};

/// `dispatcher` on an HTTP `Server`.
pub async fn http(dispatcher: impl Dispatcher) -> SocketAddr {
    let server = Server::bind("127.0.0.1:0").await.expect("bind");
    let addr = server.local_addr().expect("addr");
    tokio::spawn(server.serve_with_shutdown(dispatcher, std::future::pending()));
    addr
}

/// JSON-RPC, HTTP+JSON and gRPC listeners for `handler`, in that order.
pub async fn three(handler: &Arc<RequestHandler>) -> (SocketAddr, SocketAddr, SocketAddr) {
    let jsonrpc = http(JsonRpcDispatcher::new(Arc::clone(handler))).await;
    let rest = http(RestDispatcher::new(Arc::clone(handler))).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let grpc = listener.local_addr().expect("addr");
    tokio::spawn(
        GrpcDispatcher::new(Arc::clone(handler), GrpcConfig::default())
            .serve_with_shutdown(listener, std::future::pending()),
    );
    (jsonrpc, rest, grpc)
}
