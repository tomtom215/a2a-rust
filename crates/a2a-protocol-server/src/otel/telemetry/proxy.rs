// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! HTTP proxies for OTLP export: `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and
//! `NO_PROXY`, as curl, Go's `net/http` and reqwest read them.
//!
//! The export client dialled every collector directly and ignored all four.
//! Measured 2026-10-26 against `hello-agent`: with `HTTP_PROXY` set and an
//! endpoint only the proxy could resolve, the proxy received nothing. In a
//! network whose only way out is a proxy, telemetry exported nothing and said
//! nothing.
//!
//! The rules are `hyper-util`'s [`Matcher`] (curl's `NO_PROXY` semantics:
//! comma-separated hosts, domain suffixes, CIDR ranges, `*`), fed from the
//! injected [`Env`] so they are testable without writing the process
//! environment. What each destination does:
//!
//! - `https://` endpoint, HTTP proxy: an `HTTP CONNECT` tunnel, TLS inside it,
//!   so the collector's certificate is still verified end to end.
//! - `http://` endpoint, HTTP proxy, OTLP/HTTP: a forward proxy request, the
//!   absolute URI in the request line and `Proxy-Authorization` from the
//!   proxy URL's userinfo.
//! - `http://` endpoint, HTTP proxy, OTLP/gRPC: a tunnel, because HTTP/2
//!   cannot be forwarded as a plain request.
//! - A proxy URL that is not `http://` (`https://`, `socks5://`): not
//!   supported. The export goes direct and says so once, rather than
//!   refusing to start over a variable that is set for every other tool in
//!   the process.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::connect::proxy::Tunnel;
use hyper_util::client::legacy::connect::{Connected, Connection, HttpConnector};
use hyper_util::client::proxy::matcher::{Intercept, Matcher};
use hyper_util::rt::TokioIo;
use pin_project_lite::pin_project;
use tokio::net::TcpStream;

use super::config::Env;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Where the proxy rules come from.
#[derive(Clone, Debug, Default)]
pub(super) enum ProxySetting {
    /// `ALL_PROXY`, `HTTPS_PROXY`, `HTTP_PROXY` and `NO_PROXY`, in either case.
    #[default]
    Environment,
    /// This URL for every destination, in code. `NO_PROXY` does not apply:
    /// a proxy chosen in code is explicit, and must not change with the
    /// machine it runs on (the integration tests found that it did, on a host
    /// whose `NO_PROXY` names `localhost`).
    Url(String),
    /// No proxy, whatever the environment says.
    Off,
}

/// The first of `names` the environment sets.
fn first(env: &Env, names: &[&str]) -> String {
    names.iter().find_map(|n| env(n)).unwrap_or_default()
}

/// `NO_PROXY` with each bracketed IPv6 entry unbracketed (`[::1]` as
/// `::1`, `[fd00::]/8` as `fd00::/8`). `hyper-util` matches an IPv6
/// destination only against the bare address, so `[::1]`, the form a URL
/// writes it in, otherwise exempted nothing.
fn unbracketed(no_proxy: &str) -> String {
    no_proxy
        .split(',')
        .map(|entry| {
            let entry = entry.trim();
            match entry.strip_prefix('[').and_then(|e| e.split_once(']')) {
                Some((address, rest)) => format!("{address}{rest}"),
                None => entry.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The rules `setting` and `env` give.
pub(super) fn matcher(env: &Env, setting: &ProxySetting) -> Matcher {
    // CGI: a request header can set `HTTP_PROXY` there (httpoxy), so, like
    // curl and hyper-util's own `from_env`, no proxy is used.
    if env("REQUEST_METHOD").is_some() {
        return Matcher::builder().build();
    }
    match setting {
        ProxySetting::Off => Matcher::builder().build(),
        ProxySetting::Url(url) => Matcher::builder().all(url.as_str()).build(),
        ProxySetting::Environment => Matcher::builder()
            .all(first(env, &["ALL_PROXY", "all_proxy"]))
            .http(first(env, &["HTTP_PROXY", "http_proxy"]))
            .https(first(env, &["HTTPS_PROXY", "https_proxy"]))
            .no(unbracketed(&first(env, &["NO_PROXY", "no_proxy"])))
            .build(),
    }
}

/// The proxy that carries a request to `dst`, if one applies and is an
/// `http://` proxy, the only kind this module speaks.
pub(super) fn intercept(rules: &Matcher, dst: &http::Uri) -> Option<Intercept> {
    rules
        .intercept(dst)
        .filter(|i| i.uri().scheme_str() == Some("http"))
}

/// Whether a proxy applies to `dst` but is of a kind not supported, so the
/// caller can say so.
fn unsupported(rules: &Matcher, dst: &http::Uri) -> Option<String> {
    let found = rules.intercept(dst)?;
    (found.uri().scheme_str() != Some("http"))
        .then(|| found.uri().scheme_str().unwrap_or("").to_owned())
}

pin_project! {
    /// A connection the export client uses: straight to the collector or
    /// through a CONNECT tunnel (`Stream`, TLS where the scheme says so), or
    /// plain HTTP to a forward proxy (`Forwarded`).
    #[project = ConnProj]
    pub(super) enum Conn {
        Stream { #[pin] inner: hyper_rustls::MaybeHttpsStream<TokioIo<TcpStream>> },
        Forwarded { #[pin] inner: TokioIo<TcpStream> },
    }
}

impl Connection for Conn {
    fn connected(&self) -> Connected {
        match self {
            Self::Stream { inner } => inner.connected(),
            // The client must send the absolute URI to a forward proxy.
            Self::Forwarded { inner } => inner.connected().proxy(true),
        }
    }
}

impl Read for Conn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.project() {
            ConnProj::Stream { inner } => inner.poll_read(cx, buf),
            ConnProj::Forwarded { inner } => inner.poll_read(cx, buf),
        }
    }
}

impl Write for Conn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.project() {
            ConnProj::Stream { inner } => inner.poll_write(cx, buf),
            ConnProj::Forwarded { inner } => inner.poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.project() {
            ConnProj::Stream { inner } => inner.poll_flush(cx),
            ConnProj::Forwarded { inner } => inner.poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.project() {
            ConnProj::Stream { inner } => inner.poll_shutdown(cx),
            ConnProj::Forwarded { inner } => inner.poll_shutdown(cx),
        }
    }
}

/// A connector for the OTLP/HTTP client that sends each destination straight
/// or through the proxy the rules name for it.
#[derive(Clone)]
pub(super) struct ProxyConnector {
    tls: rustls::ClientConfig,
    direct: hyper_rustls::HttpsConnector<HttpConnector>,
    tcp: HttpConnector,
    rules: Arc<Matcher>,
    said_unsupported: Arc<AtomicBool>,
}

impl ProxyConnector {
    pub(super) fn new(tls: &rustls::ClientConfig, rules: Arc<Matcher>) -> Self {
        let direct = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls.clone())
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        let mut tcp = HttpConnector::new();
        tcp.enforce_http(false);
        Self {
            tls: tls.clone(),
            direct,
            tcp,
            rules,
            said_unsupported: Arc::new(AtomicBool::new(false)),
        }
    }

    /// TLS to the collector over a `CONNECT` tunnel through `proxy`.
    fn tunnelled_tls(
        &self,
        proxy: &Intercept,
    ) -> hyper_rustls::HttpsConnector<Tunnel<HttpConnector>> {
        hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(self.tls.clone())
            .https_only()
            .enable_http1()
            .enable_http2()
            .wrap_connector(tunnel(proxy, self.tcp.clone()))
    }
}

/// A `CONNECT` tunnel through `proxy`, authenticated when its URL carried a
/// user.
pub(super) fn tunnel(proxy: &Intercept, tcp: HttpConnector) -> Tunnel<HttpConnector> {
    let tunnel = Tunnel::new(proxy.uri().clone(), tcp);
    match proxy.basic_auth() {
        Some(auth) => tunnel.with_auth(auth.clone()),
        None => tunnel,
    }
}

impl tower_service::Service<http::Uri> for ProxyConnector {
    type Response = Conn;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Conn, BoxError>> + Send>>;

    // Equivalent mutant: cargo-mutants replaces the body with
    // `Poll::from(Ok(()))`, and `Poll::from(t)` is `Poll::Ready(t)`, the value
    // written here. No test can distinguish the two (ADR 0006).
    #[mutants::skip]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, dst: http::Uri) -> Self::Future {
        let https = dst.scheme_str() == Some("https");
        match (intercept(&self.rules, &dst), https) {
            (None, _) => {
                if let Some(scheme) = unsupported(&self.rules, &dst)
                    && !self.said_unsupported.swap(true, Ordering::Relaxed)
                {
                    tracing::warn!(
                        "OTLP export: the proxy for {dst} is `{scheme}://`, which is not \
                         supported (only `http://` proxies are); exporting directly"
                    );
                }
                let mut direct = self.direct.clone();
                Box::pin(async move {
                    let inner = direct.call(dst).await?;
                    Ok(Conn::Stream { inner })
                })
            }
            (Some(proxy), true) => {
                let mut through = self.tunnelled_tls(&proxy);
                Box::pin(async move {
                    let inner = through.call(dst).await?;
                    Ok(Conn::Stream { inner })
                })
            }
            (Some(proxy), false) => {
                let mut tcp = self.tcp.clone();
                let proxy_uri = proxy.uri().clone();
                Box::pin(async move {
                    let inner = tcp.call(proxy_uri).await?;
                    Ok(Conn::Forwarded { inner })
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
