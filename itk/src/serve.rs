// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The agent's HTTP listener. Moved out of `main.rs`, which is over the
//! 500-line limit.

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};

/// Serves JSON-RPC (POST /), REST (other paths), and the agent card on one
/// listener — the same routing as the reference agents' HTTP port.
pub async fn serve_http(
    handler: Arc<a2a_protocol_server::handler::RequestHandler>,
    port: u16,
) -> std::io::Result<SocketAddr> {
    let jsonrpc = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));
    let rest = Arc::new(RestDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let _ = stream.set_nodelay(true);
            let io = hyper_util::rt::TokioIo::new(stream);
            let jsonrpc = Arc::clone(&jsonrpc);
            let rest = Arc::clone(&rest);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                    let jsonrpc = Arc::clone(&jsonrpc);
                    let rest = Arc::clone(&rest);
                    async move {
                        // The ITK runner (and the reference agents' layout)
                        // target v1.0 JSON-RPC at /jsonrpc as well as /.
                        let path = req.uri().path();
                        let is_jsonrpc = req.method() == hyper::Method::POST
                            && (path == "/"
                                || path.is_empty()
                                || path == "/jsonrpc"
                                || path == "/jsonrpc/");
                        if is_jsonrpc {
                            Ok::<_, std::convert::Infallible>(jsonrpc.dispatch(req).await)
                        } else {
                            Ok(rest.dispatch(req).await)
                        }
                    }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection_with_upgrades(io, service)
                .await;
            });
        }
    });
    Ok(addr)
}
