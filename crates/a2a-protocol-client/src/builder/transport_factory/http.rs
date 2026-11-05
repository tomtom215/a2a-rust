// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The HTTP bindings' transports, and the HTTP client each sends through.

use super::{GRPC_NOT_SYNC, PoolHandle};
use crate::builder::selection::canonical_binding;
use crate::config::{BINDING_GRPC, BINDING_HTTP_JSON, BINDING_JSONRPC, ClientConfig};
use crate::error::{ClientError, ClientResult};
use crate::transport::{JsonRpcTransport, RestTransport, Transport};

/// Constructs the HTTP transport for `binding` at `endpoint`.
///
/// `binding` is matched ignoring ASCII case. `HTTP+JSON` is the spec name
/// for the REST binding and `REST` the legacy alias; a card published by an
/// official Go/Python/Java SDK advertises the spec name, so both resolve to
/// the REST transport.
pub(super) fn http_transport(
    config: &ClientConfig,
    pool: Option<&PoolHandle>,
    binding: &str,
    endpoint: &str,
) -> ClientResult<Box<dyn Transport>> {
    build_http_transport(config, binding, endpoint, || {
        http_client(config, pool, endpoint)
    })
}

/// [`http_transport`] with the HTTP client `client` makes, asked for only
/// once the binding is known to be one this builds.
pub(super) fn build_http_transport(
    config: &ClientConfig,
    binding: &str,
    endpoint: &str,
    client: impl FnOnce() -> ClientResult<crate::transport::HttpClient>,
) -> ClientResult<Box<dyn Transport>> {
    match canonical_binding(binding) {
        Some(BINDING_JSONRPC) => Ok(Box::new(
            JsonRpcTransport::from_client(
                endpoint,
                client()?,
                config.request_timeout,
                config.stream_connect_timeout,
            )?
            .with_max_response_size(config.max_response_size)
            .with_send_timeout(config.send_timeout),
        )),
        Some(BINDING_HTTP_JSON) => Ok(Box::new(
            RestTransport::from_client(
                endpoint,
                client()?,
                config.request_timeout,
                config.stream_connect_timeout,
            )?
            .with_max_response_size(config.max_response_size)
            .with_send_timeout(config.send_timeout),
        )),
        Some(BINDING_GRPC) => Err(ClientError::Config(GRPC_NOT_SYNC.into())),
        _ => Err(ClientError::Config(format!(
            "unknown protocol binding: {binding}"
        ))),
    }
}

/// The HTTP client for a transport to `endpoint`: the pool's, when the
/// builder was given one, and otherwise one of its own as `config.tls` asks.
pub(super) fn http_client(
    config: &ClientConfig,
    pool: Option<&PoolHandle>,
    endpoint: &str,
) -> ClientResult<crate::transport::HttpClient> {
    use crate::config::TlsConfig;

    #[cfg(feature = "tls-rustls")]
    if let Some(pool) = pool {
        return Ok(pool.client());
    }
    // Without `tls-rustls` a pool cannot exist.
    #[cfg(not(feature = "tls-rustls"))]
    let _ = pool;

    let https = endpoint
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"));
    if matches!(config.tls, TlsConfig::Disabled) && https {
        return Err(ClientError::InvalidEndpoint(format!(
            "{endpoint} is https:// and TLS is disabled (ClientBuilder::without_tls)"
        )));
    }
    #[cfg(feature = "tls-rustls")]
    {
        let tls = match &config.tls {
            TlsConfig::Custom(tls) => (**tls).clone(),
            _ => crate::tls::default_tls_config(),
        };
        Ok(crate::tls::build_https_client_with_connect_timeout(
            tls,
            config.connection_timeout,
        ))
    }
    #[cfg(not(feature = "tls-rustls"))]
    Ok(crate::transport::plain_http_client(
        config.connection_timeout,
    ))
}
