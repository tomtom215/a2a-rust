// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The connector of the plaintext build's push client (no `tls-rustls`): an
//! `http://` destination goes straight to its pinned or resolved address, or,
//! when the proxy rules name an `http://` proxy for it, to that proxy as a
//! forward request (the absolute URI in the request line, as curl sends it).
//!
//! Until 0.16.0 this build's push sender used a bare `HttpConnector` and read
//! no proxy variable, so in a network whose only way out is a proxy every
//! delivery failed (audit NEW-b-3). The TLS build's connector (`proxy`) does
//! the same for `http://` and also tunnels `https://`, which this build
//! refuses before connecting.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::client::proxy::matcher::Matcher;
use hyper_util::rt::TokioIo;
use pin_project_lite::pin_project;
use tokio::net::TcpStream;
use tower_service::Service;

use super::proxy::{intercept, unsupported};
use super::resolver::Tcp;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

pin_project! {
    /// A plaintext connection: to the destination, or to a forward proxy,
    /// which the client must then send the absolute URI.
    pub struct PlainConn {
        #[pin]
        inner: TokioIo<TcpStream>,
        forwarded: bool,
    }
}

impl Connection for PlainConn {
    fn connected(&self) -> Connected {
        self.inner.connected().proxy(self.forwarded)
    }
}

impl Read for PlainConn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.project().inner.poll_read(cx, buf)
    }
}

impl Write for PlainConn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.project().inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.project().inner.poll_shutdown(cx)
    }
}

/// Dials each destination straight, or through the `http://` proxy the rules
/// name for it.
#[derive(Clone)]
pub struct PlainConnector {
    tcp: Tcp,
    rules: Arc<Matcher>,
    said_unsupported: Arc<AtomicBool>,
}

impl PlainConnector {
    /// Dials with `tcp`, already tuned (connect timeout, keepalive,
    /// `TCP_NODELAY`), by `rules`.
    pub fn new(rules: Arc<Matcher>, tcp: Tcp) -> Self {
        Self {
            tcp,
            rules,
            said_unsupported: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Service<http::Uri> for PlainConnector {
    type Response = PlainConn;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<PlainConn, BoxError>> + Send>>;

    // Equivalent mutant: cargo-mutants replaces the body with
    // `Poll::from(Ok(()))`, and `Poll::from(t)` is `Poll::Ready(t)`, the value
    // written here. No test can distinguish the two (ADR 0006).
    #[mutants::skip]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, dst: http::Uri) -> Self::Future {
        let mut tcp = self.tcp.clone();
        let (target, forwarded) = if let Some(proxy) = intercept(&self.rules, &dst) {
            (proxy.uri().clone(), true)
        } else {
            if let Some(_scheme) = unsupported(&self.rules, &dst)
                && !self.said_unsupported.swap(true, Ordering::Relaxed)
            {
                trace_warn!(
                    "the proxy for {dst} is `{_scheme}://`, which is not supported \
                     (only `http://` proxies are); connecting directly"
                );
            }
            (dst, false)
        };
        Box::pin(async move {
            let inner = tcp.call(target).await?;
            Ok(PlainConn { inner, forwarded })
        })
    }
}
