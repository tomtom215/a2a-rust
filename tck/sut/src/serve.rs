// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The SUT's HTTP listener. Moved out of `main.rs`, which is over the
//! 500-line limit.

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};

/// Serves JSON-RPC and HTTP+JSON on one socket.
///
/// The TCK reads both bindings' URLs from the agent card and, by default,
/// points them at the same host — so a single listener must answer both. The
/// REST dispatcher owns the routed paths (`/message:send`, `/tasks/…`) and
/// JSON-RPC owns `POST /`, which is exactly how they partition.
pub async fn serve(addr: SocketAddr, handler: Arc<a2a_protocol_server::handler::RequestHandler>) {
    let jsonrpc = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));
    let rest = Arc::new(RestDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind SUT listener");
    eprintln!("a2a-rust TCK SUT listening on http://{addr}");

    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let io = hyper_util::rt::TokioIo::new(stream);
        let jsonrpc = Arc::clone(&jsonrpc);
        let rest = Arc::clone(&rest);
        tokio::spawn(async move {
            let service =
                hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let jsonrpc = Arc::clone(&jsonrpc);
                    let rest = Arc::clone(&rest);
                    async move {
                        // JSON-RPC is `POST /`; everything else is REST-routed.
                        let is_jsonrpc =
                            req.method() == hyper::Method::POST && req.uri().path() == "/";
                        let resp = if is_jsonrpc {
                            jsonrpc.dispatch(req).await
                        } else {
                            rest.dispatch(req).await
                        };
                        Ok::<_, std::convert::Infallible>(resp)
                    }
                });
            let _ =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(io, service)
                    .await;
        });
    }
}
