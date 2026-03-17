// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for the `serve` module (`serve_with_addr`).

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Incoming;

use a2a_protocol_server::serve::{serve_with_addr, DispatchResponse, Dispatcher};

// ── Dummy dispatcher ────────────────────────────────────────────────────────

struct EchoDispatcher;

impl Dispatcher for EchoDispatcher {
    fn dispatch(
        &self,
        _req: hyper::Request<Incoming>,
    ) -> Pin<Box<dyn Future<Output = DispatchResponse> + Send + '_>> {
        Box::pin(async {
            let body = Full::new(Bytes::from("ok"));
            hyper::Response::new(BoxBody::new(body.map_err(|_: Infallible| unreachable!())))
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Open an HTTP/1.1 connection to `addr`, send a GET /, return the response.
async fn send_get(addr: std::net::SocketAddr) -> hyper::Response<Incoming> {
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let io = hyper_util::rt::TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(conn);

    let req = hyper::Request::get("/")
        .body(Empty::<Bytes>::new())
        .unwrap();
    sender.send_request(req).await.unwrap()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn serve_with_addr_binds_and_accepts() {
    let addr = serve_with_addr("127.0.0.1:0", EchoDispatcher)
        .await
        .expect("serve_with_addr should bind");

    let resp = send_get(addr).await;
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"ok");
}

#[tokio::test]
async fn serve_with_addr_port_zero_returns_real_port() {
    let addr = serve_with_addr("127.0.0.1:0", EchoDispatcher)
        .await
        .expect("serve_with_addr should bind");

    assert_ne!(addr.port(), 0, "returned port should not be zero");
}

#[tokio::test]
async fn serve_with_addr_handles_multiple_connections() {
    let addr = serve_with_addr("127.0.0.1:0", EchoDispatcher)
        .await
        .expect("serve_with_addr should bind");

    for i in 0..3 {
        let resp = send_get(addr).await;
        assert_eq!(resp.status(), 200, "request {i} should succeed");

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"ok", "request {i} body should be 'ok'");
    }
}
