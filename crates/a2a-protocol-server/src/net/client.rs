// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The pooled HTTP client push delivery, JWKS and OIDC discovery and the model
//! client share: the environment's proxy, the environment's roots, and a pool
//! tuned for what sits in front of a receiver.
//!
//! Without `tls-rustls` it is the push sender's plaintext client: the same
//! proxy rules, pins and pool, over `net::plain`'s connector,
//! which forwards `http://` through an `http://` proxy (audit NEW-b-3).

use bytes::Bytes;
use http_body_util::Full;
use hyper_util::client::legacy::{Client, ResponseFuture};
use hyper_util::client::proxy::matcher::Matcher;
use hyper_util::rt::TokioExecutor;
use std::sync::Arc;

#[cfg(not(feature = "tls-rustls"))]
use super::plain::PlainConnector as Connector;
#[cfg(feature = "tls-rustls")]
use super::proxy::ProxyConnector as Connector;
use super::proxy::{self, ProxySetting};
use super::resolver::{PinGuard, PinResolver, Tcp};
use crate::tcp::{POOL_IDLE_TIMEOUT, tune};

/// A client that goes through the proxy `HTTPS_PROXY`, `HTTP_PROXY`,
/// `ALL_PROXY` and `NO_PROXY` name: HTTPS-capable with `tls-rustls`,
/// plaintext without.
#[derive(Clone)]
pub struct HttpsClient {
    inner: Client<Connector, Full<Bytes>>,
    rules: Arc<Matcher>,
    resolver: PinResolver,
}

/// The proxy rules the process environment gives.
fn environment_rules() -> Arc<Matcher> {
    Arc::new(proxy::matcher(
        &proxy::process_env,
        &ProxySetting::Environment,
    ))
}

impl HttpsClient {
    /// A client verifying servers against `tls`, with the proxy rules the
    /// process environment gives.
    #[cfg(feature = "tls-rustls")]
    pub fn new(tls: &rustls::ClientConfig) -> Self {
        Self::with_rules(tls, environment_rules())
    }

    /// A client with explicit proxy `rules`: what the tests give it.
    #[cfg(feature = "tls-rustls")]
    pub fn with_rules(tls: &rustls::ClientConfig, rules: Arc<Matcher>) -> Self {
        Self::assemble(rules, |rules, tcp| Connector::with_tcp(tls, rules, tcp))
    }

    /// A plaintext client with the proxy rules the process environment gives.
    #[cfg(not(feature = "tls-rustls"))]
    pub fn plain() -> Self {
        Self::plain_with_rules(environment_rules())
    }

    /// A plaintext client with explicit proxy `rules`: what the tests give it.
    #[cfg(not(feature = "tls-rustls"))]
    pub fn plain_with_rules(rules: Arc<Matcher>) -> Self {
        Self::assemble(rules, Connector::new)
    }

    fn assemble(
        rules: Arc<Matcher>,
        connector: impl FnOnce(Arc<Matcher>, Tcp) -> Connector,
    ) -> Self {
        let resolver = PinResolver::default();
        let tcp = tcp_connector(resolver.clone());
        let connector = connector(Arc::clone(&rules), tcp);
        let inner = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(POOL_IDLE_TIMEOUT)
            .build(connector);
        Self {
            inner,
            rules,
            resolver,
        }
    }

    /// Whether a request to `uri` goes through a proxy: one the rules name
    /// for it, of a kind this crate speaks (`http://`).
    pub fn proxied(&self, uri: &hyper::Uri) -> bool {
        proxy::intercept(&self.rules, uri).is_some()
    }

    /// Fixes `host` to `addrs` until the guard is dropped: a connection this
    /// client opens to that name, for an `https://` URL as well as an
    /// `http://` one, goes to those addresses and to nothing the system
    /// resolver would say, and the certificate is still checked against the
    /// name. Does not reach a request sent through a proxy, which resolves
    /// the name itself ([`proxied`](Self::proxied)).
    pub fn pin(&self, host: &str, addrs: Vec<std::net::SocketAddr>) -> PinGuard {
        self.resolver.pin(host, addrs)
    }

    /// Whether `host` is pinned by a delivery that has not ended.
    #[cfg(test)]
    pub(crate) fn is_pinned(&self, host: &str) -> bool {
        self.resolver.is_pinned(host)
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

/// The TCP connector the client dials with: tuned (`crate::tcp`) and
/// resolving through `resolver`.
fn tcp_connector(resolver: PinResolver) -> Tcp {
    let mut tcp = Tcp::new_with_resolver(resolver);
    tune(&mut tcp);
    tcp
}

impl std::fmt::Debug for HttpsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpsClient").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
