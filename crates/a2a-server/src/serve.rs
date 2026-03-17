// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Server startup helpers.
//!
//! Reduces the ~25 lines of hyper boilerplate typically needed to start an
//! A2A HTTP server down to a single function call.
//!
//! # Example
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use a2a_protocol_server::serve::serve;
//! use a2a_protocol_server::dispatch::JsonRpcDispatcher;
//! use a2a_protocol_server::RequestHandlerBuilder;
//! # struct MyExecutor;
//! # impl a2a_protocol_server::executor::AgentExecutor for MyExecutor {
//! #     fn execute<'a>(&'a self, _ctx: &'a a2a_protocol_server::request_context::RequestContext,
//! #         _queue: &'a dyn a2a_protocol_server::streaming::EventQueueWriter,
//! #     ) -> std::pin::Pin<Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>> {
//! #         Box::pin(async { Ok(()) })
//! #     }
//! # }
//!
//! # async fn example() -> std::io::Result<()> {
//! let handler = Arc::new(
//!     RequestHandlerBuilder::new(MyExecutor)
//!         .build()
//!         .expect("build handler"),
//! );
//!
//! let dispatcher = JsonRpcDispatcher::new(handler);
//! serve("127.0.0.1:3000", dispatcher).await?;
//! # Ok(())
//! # }
//! ```

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;

// ── Types ────────────────────────────────────────────────────────────────────

/// The HTTP response type returned by dispatchers.
pub type DispatchResponse = hyper::Response<BoxBody<Bytes, Infallible>>;

// ── Dispatcher trait ─────────────────────────────────────────────────────────

/// Trait for types that can dispatch HTTP requests to an A2A handler.
///
/// Implemented by both [`JsonRpcDispatcher`](crate::JsonRpcDispatcher) and
/// [`RestDispatcher`](crate::RestDispatcher).
pub trait Dispatcher: Send + Sync + 'static {
    /// Dispatches an HTTP request and returns a response.
    fn dispatch(
        &self,
        req: hyper::Request<Incoming>,
    ) -> Pin<Box<dyn Future<Output = DispatchResponse> + Send + '_>>;
}

// ── serve ────────────────────────────────────────────────────────────────────

/// Starts an HTTP server that dispatches requests using the given dispatcher.
///
/// Binds a TCP listener on `addr`, accepts connections, and serves each one
/// using a hyper auto-connection builder. This eliminates the ~25 lines of
/// boilerplate that every A2A agent otherwise needs.
///
/// The server runs until the listener encounters an I/O error. Each connection
/// is served in a separate Tokio task.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the TCP listener fails to bind.
///
/// # Example
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use a2a_protocol_server::serve::serve;
/// use a2a_protocol_server::dispatch::JsonRpcDispatcher;
/// use a2a_protocol_server::RequestHandlerBuilder;
/// # struct MyExecutor;
/// # impl a2a_protocol_server::executor::AgentExecutor for MyExecutor {
/// #     fn execute<'a>(&'a self, _ctx: &'a a2a_protocol_server::request_context::RequestContext,
/// #         _queue: &'a dyn a2a_protocol_server::streaming::EventQueueWriter,
/// #     ) -> std::pin::Pin<Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>> {
/// #         Box::pin(async { Ok(()) })
/// #     }
/// # }
///
/// # async fn example() -> std::io::Result<()> {
/// let handler = Arc::new(
///     RequestHandlerBuilder::new(MyExecutor)
///         .build()
///         .expect("build handler"),
/// );
///
/// let dispatcher = JsonRpcDispatcher::new(handler);
/// serve("127.0.0.1:3000", dispatcher).await?;
/// # Ok(())
/// # }
/// ```
pub async fn serve(
    addr: impl tokio::net::ToSocketAddrs,
    dispatcher: impl Dispatcher,
) -> std::io::Result<()> {
    let dispatcher = Arc::new(dispatcher);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    trace_info!(
        addr = %listener.local_addr().unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0))),
        "A2A server listening"
    );

    loop {
        let (stream, _peer) = listener.accept().await?;
        let io = hyper_util::rt::TokioIo::new(stream);
        let dispatcher = Arc::clone(&dispatcher);

        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let d = Arc::clone(&dispatcher);
                async move { Ok::<_, Infallible>(d.dispatch(req).await) }
            });
            let _ =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(io, service)
                    .await;
        });
    }
}

/// Starts an HTTP server and returns the bound [`SocketAddr`].
///
/// Like [`serve`], but binds before entering the accept loop and returns the
/// actual address (useful when binding to port `0` for tests).
///
/// # Errors
///
/// Returns [`std::io::Error`] if the TCP listener fails to bind.
pub async fn serve_with_addr(
    addr: impl tokio::net::ToSocketAddrs,
    dispatcher: impl Dispatcher,
) -> std::io::Result<SocketAddr> {
    let dispatcher = Arc::new(dispatcher);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    trace_info!(%local_addr, "A2A server listening");

    tokio::spawn(async move {
        loop {
            let Ok((stream, _peer)) = listener.accept().await else {
                break;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);

            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
                    async move { Ok::<_, Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });

    Ok(local_addr)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use http_body_util::{BodyExt, Empty};
    use hyper_util::client::legacy::Client;
    use hyper_util::rt::TokioExecutor;

    struct MockDispatcher;

    impl Dispatcher for MockDispatcher {
        fn dispatch(
            &self,
            _req: hyper::Request<Incoming>,
        ) -> Pin<Box<dyn Future<Output = DispatchResponse> + Send + '_>> {
            Box::pin(async {
                let body = http_body_util::Full::new(Bytes::from("ok"));
                hyper::Response::new(BoxBody::new(body.map_err(|e| match e {})))
            })
        }
    }

    #[tokio::test]
    async fn serve_with_addr_returns_bound_address() {
        let addr = serve_with_addr("127.0.0.1:0", MockDispatcher)
            .await
            .expect("server should bind");

        assert_ne!(addr.port(), 0, "should bind to a real port");
        assert!(addr.ip().is_loopback());

        let client = Client::builder(TokioExecutor::new()).build_http::<Empty<Bytes>>();
        let resp = client
            .get(format!("http://{addr}/").parse().unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn serve_with_addr_handles_multiple_connections() {
        let addr = serve_with_addr("127.0.0.1:0", MockDispatcher)
            .await
            .expect("server should bind");

        let client = Client::builder(TokioExecutor::new()).build_http::<Empty<Bytes>>();

        for i in 0..3 {
            let resp = client
                .get(format!("http://{addr}/").parse().unwrap())
                .await
                .unwrap_or_else(|e| panic!("request {i} failed: {e}"));
            let body = resp.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(&body[..], b"ok", "request {i} returned unexpected body");
        }
    }
}
