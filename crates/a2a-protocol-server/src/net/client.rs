// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The pooled HTTP client push delivery, JWKS and OIDC discovery and the model
//! client share: the environment's proxy, the environment's roots, and a pool
//! tuned for what sits in front of a receiver.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::{Client, ResponseFuture};
use hyper_util::client::proxy::matcher::Matcher;
use hyper_util::rt::TokioExecutor;

use super::proxy::{self, ProxyConnector, ProxySetting};

/// How long a TCP connect may take. Without a bound a black-holed address
/// holds the caller for the operating system's SYN retry schedule, minutes
/// (audit B-30).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// TCP keepalive on a pooled connection, so a path that drops idle flows (a
/// NAT, a load balancer) is noticed instead of reused.
pub const TCP_KEEPALIVE: Duration = Duration::from_secs(30);

/// How long an idle pooled connection is kept. Below the 60 s idle timeout of
/// common load balancers (AWS ALB, nginx's keepalive) and this crate's
/// server's 75 s: a connection the peer has already closed is not reused
/// (audit B-07, B-30, N-B-3). hyper-util's default is 90 s.
pub const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(50);

/// An HTTPS-capable client that goes through the proxy `HTTPS_PROXY`,
/// `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY` name.
#[derive(Clone)]
pub struct HttpsClient {
    inner: Client<ProxyConnector, Full<Bytes>>,
    rules: Arc<Matcher>,
}

impl HttpsClient {
    /// A client verifying servers against `tls`, with the proxy rules the
    /// process environment gives.
    pub fn new(tls: &rustls::ClientConfig) -> Self {
        Self::with_rules(
            tls,
            Arc::new(proxy::matcher(
                &proxy::process_env,
                &ProxySetting::Environment,
            )),
        )
    }

    /// A client with explicit proxy `rules`: what the tests give it.
    pub fn with_rules(tls: &rustls::ClientConfig, rules: Arc<Matcher>) -> Self {
        let mut tcp = HttpConnector::new();
        tcp.set_nodelay(true);
        tcp.set_connect_timeout(Some(CONNECT_TIMEOUT));
        tcp.set_keepalive(Some(TCP_KEEPALIVE));
        let connector = ProxyConnector::with_tcp(tls, Arc::clone(&rules), tcp);
        let inner = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(POOL_IDLE_TIMEOUT)
            .build(connector);
        Self { inner, rules }
    }

    /// Sends `req`. A plain `http://` request through a forward proxy whose
    /// URL carries a user gets its `Proxy-Authorization`; a tunnelled one is
    /// authenticated in the `CONNECT`.
    pub fn request(&self, mut req: hyper::Request<Full<Bytes>>) -> ResponseFuture {
        if req.uri().scheme_str() == Some("http")
            && let Some(auth) =
                proxy::intercept(&self.rules, req.uri()).and_then(|p| p.basic_auth().cloned())
        {
            req.headers_mut()
                .insert(hyper::header::PROXY_AUTHORIZATION, auth);
        }
        self.inner.request(req)
    }
}

impl std::fmt::Debug for HttpsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpsClient").finish_non_exhaustive()
    }
}
