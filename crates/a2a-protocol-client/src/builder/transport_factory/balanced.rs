// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What `build()` adds for ADR 0050: the balanced transport, and the pool's
//! per-origin state under a client that is not balanced.

use std::sync::Arc;

use super::{ClientBuilder, build_http_transport, http_client};
use crate::balance::{BalancePolicy, BalancedTransport, Endpoint, EndpointSource};
use crate::config::{BINDING_HTTP_JSON, BINDING_JSONRPC, ClientConfig};
use crate::error::{ClientError, ClientResult};
use crate::origin::{OriginState, OriginTransport, Origins, origin_key};
use crate::transport::{HttpClient, Transport};

use super::super::selection::canonical_binding;
use super::PoolHandle;

/// The scheme of `url`, lower-cased; empty when it has none.
fn scheme_of(url: &str) -> String {
    url.split_once("://")
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .unwrap_or_default()
}

impl ClientBuilder {
    /// Balancing is for the HTTP bindings this builder builds: a custom
    /// transport is a conflict, and so is another binding.
    pub(super) fn check_balancing(&self) -> ClientResult<()> {
        if self.balancing.is_none() {
            return Ok(());
        }
        if self.transport_override.is_some() {
            return Err(ClientError::Config(
                "ClientBuilder::with_balancing and ClientBuilder::with_custom_transport were \
                 both called, but a custom transport is one connection the builder cannot \
                 spread; drop one of the two"
                    .into(),
            ));
        }
        let binding = self.preferred_binding.as_deref().unwrap_or(BINDING_JSONRPC);
        if !matches!(
            canonical_binding(binding),
            Some(BINDING_JSONRPC | BINDING_HTTP_JSON)
        ) {
            return Err(ClientError::Config(format!(
                "ClientBuilder::with_balancing balances the HTTP bindings (JSON-RPC, HTTP+JSON), \
                 and this builder is for {binding}"
            )));
        }
        Ok(())
    }

    /// The pool's per-origin state, when the builder has a pool.
    #[cfg(feature = "tls-rustls")]
    fn pool_origins(&self) -> Option<Arc<Origins>> {
        self.pool
            .as_ref()
            .map(|pool| Arc::clone(pool.origin_registry()))
    }

    /// Without `tls-rustls` there is no pool: `PoolHandle` is `Infallible`.
    // No test can kill cargo-mutants' replacements here (ADR 0006): the gate
    // builds with every feature, where this function is compiled out, and in
    // the one build that has it (`--no-default-features`, checked 2026-11-06)
    // `Some(Arc::new(Default::default()))` does not compile (E0277: `Origins`
    // has no `Default`) and `None` is what the body returns, since `self.pool`
    // is an `Option<Infallible>` and has no `Some` to map.
    #[cfg(not(feature = "tls-rustls"))]
    #[mutants::skip]
    fn pool_origins(&self) -> Option<Arc<Origins>> {
        self.pool.map(|never| match never {})
    }

    /// The state of this client's own origin in the pool, if the pool keeps
    /// one for it.
    fn own_origin(&self) -> Option<Arc<OriginState>> {
        let origins = self.pool_origins()?;
        origin_key(&self.endpoint, None).map(|key| origins.get(&key))
    }

    /// `transport` with the pool's circuit breaker and in-flight cap for this
    /// client's origin in front, when the pool has either.
    pub(super) fn guard_origin(&self, transport: Box<dyn Transport>) -> Box<dyn Transport> {
        if !self.pool_origins().is_some_and(|o| o.enforces()) {
            return transport;
        }
        match self.own_origin() {
            Some(origin) => Box::new(OriginTransport::new(transport, origin)),
            None => transport,
        }
    }

    /// The pool's retry ledger for this client's origin, when the pool keeps
    /// retry budgets.
    pub(super) fn shared_budget(&self) -> Option<Arc<OriginState>> {
        if !self.pool_origins().is_some_and(|o| o.has_budget()) {
            return None;
        }
        self.own_origin()
    }

    /// The transport over `source`'s endpoints.
    pub(super) fn balanced_transport(
        &self,
        source: Arc<dyn EndpointSource>,
        policy: &BalancePolicy,
    ) -> ClientResult<Box<dyn Transport>> {
        let origins = match self.pool_origins() {
            Some(_) if policy.ejection_set() => {
                return Err(ClientError::Config(
                    "BalancePolicy::with_ejection (or without_ejection) and \
                     ClientBuilder::with_pool were both given, but a pool keeps one ejection \
                     policy for every client that uses it: set it with \
                     HttpPoolBuilder::with_ejection(..), or drop one of the two"
                        .into(),
                ));
            }
            Some(origins) => origins,
            None => Arc::new(Origins::new(policy.ejection, None, None)),
        };
        let binding = self
            .preferred_binding
            .clone()
            .unwrap_or_else(|| BINDING_JSONRPC.into());
        let scheme = scheme_of(&self.endpoint);
        let config = self.config.clone();
        #[cfg(feature = "tls-rustls")]
        let pool = self.pool.clone();
        // `Option<Infallible>` is `Copy`.
        #[cfg(not(feature = "tls-rustls"))]
        let pool = self.pool;
        let factory = Arc::new(move |endpoint: &Endpoint| {
            if scheme_of(endpoint.url()) != scheme {
                return Err(ClientError::Config(format!(
                    "{} is not used: its scheme is not the client endpoint's ({scheme}://), \
                     which the plaintext check is made against",
                    crate::redact::without_userinfo(endpoint.url())
                )));
            }
            let key = origin_key(endpoint.url(), endpoint.addr()).ok_or_else(|| {
                ClientError::InvalidEndpoint(format!(
                    "{} has no host",
                    crate::redact::without_userinfo(endpoint.url())
                ))
            })?;
            let transport = build_http_transport(&config, &binding, endpoint.url(), || {
                endpoint_client(&config, pool.as_ref(), endpoint)
            })?;
            Ok((transport, origins.get(&key)))
        });
        Ok(Box::new(BalancedTransport::new(
            self.endpoint.clone(),
            source,
            factory,
            policy,
            self.peer_honours_idempotency,
        )))
    }
}

/// The HTTP client for `endpoint`: pinned to its address when it has one.
fn endpoint_client(
    config: &ClientConfig,
    pool: Option<&PoolHandle>,
    endpoint: &Endpoint,
) -> ClientResult<HttpClient> {
    let Some(addr) = endpoint.addr() else {
        return http_client(config, pool, endpoint.url());
    };
    pinned_client(config, pool, endpoint.url(), addr)
}

#[cfg(feature = "tls-rustls")]
fn pinned_client(
    config: &ClientConfig,
    pool: Option<&PoolHandle>,
    url: &str,
    addr: std::net::SocketAddr,
) -> ClientResult<HttpClient> {
    use crate::config::TlsConfig;
    let host = url
        .parse::<hyper::Uri>()
        .ok()
        .and_then(|u| u.host().map(str::to_owned))
        .ok_or_else(|| {
            ClientError::InvalidEndpoint(format!(
                "{} has no host",
                crate::redact::without_userinfo(url)
            ))
        })?;
    if let Some(pool) = pool {
        return Ok(pool.pinned_client(&host, addr));
    }
    if matches!(config.tls, TlsConfig::Disabled) && scheme_of(url) == "https" {
        return Err(ClientError::InvalidEndpoint(format!(
            "{} is https:// and TLS is disabled (ClientBuilder::without_tls)",
            crate::redact::without_userinfo(url)
        )));
    }
    let tls = match &config.tls {
        TlsConfig::Custom(tls) => (**tls).clone(),
        _ => crate::tls::default_tls_config(),
    };
    Ok(crate::tls::build_pinned_https_client(
        tls,
        config.connection_timeout,
        &(host, addr),
    ))
}

// No test can kill cargo-mutants' `Ok(Default::default())` here (ADR 0006): the
// gate builds with every feature, where this function is compiled out, and in
// the one build that has it (`--no-default-features`, checked 2026-11-06) the
// mutant does not compile (E0277: hyper-util's `Client` has no `Default`).
#[cfg(not(feature = "tls-rustls"))]
#[mutants::skip]
fn pinned_client(
    _config: &ClientConfig,
    _pool: Option<&PoolHandle>,
    url: &str,
    _addr: std::net::SocketAddr,
) -> ClientResult<HttpClient> {
    Err(ClientError::Config(format!(
        "{} is pinned to an address, which needs the tls-rustls feature",
        crate::redact::without_userinfo(url)
    )))
}

#[cfg(test)]
mod tests;
