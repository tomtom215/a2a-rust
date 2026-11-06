// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the balanced factory refuses when it pins a client to an address, and
//! which per-origin state a builder with a pool has. The balancing itself is
//! `balance/`'s and `tests/`'s.

#![cfg(feature = "tls-rustls")]

use std::net::SocketAddr;

use super::*;
use crate::config::TlsConfig;

fn addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 4433))
}

/// A URL with no host has nothing to pin an address to.
#[test]
fn a_url_without_a_host_cannot_be_pinned() {
    let err = pinned_client(&ClientConfig::default(), None, "not a url", addr())
        .expect_err("no host to pin");
    assert!(
        matches!(&err, ClientError::InvalidEndpoint(m) if m.contains("has no host")),
        "{err}"
    );
}

/// `https://` with TLS disabled is a conflict the builder names; `http://`
/// with TLS disabled is not one.
#[test]
fn https_with_tls_disabled_is_refused_and_http_is_not() {
    let config = ClientConfig {
        tls: TlsConfig::Disabled,
        ..ClientConfig::default()
    };
    let err = pinned_client(&config, None, "https://agent.example/", addr())
        .expect_err("https needs TLS");
    assert!(
        matches!(&err, ClientError::InvalidEndpoint(m) if m.contains("TLS is disabled")),
        "{err}"
    );
    assert!(pinned_client(&config, None, "http://agent.example/", addr()).is_ok());
}

/// The per-origin state a builder gives its client is its pool's, and a
/// builder with no pool has none.
#[tokio::test]
async fn a_builder_with_a_pool_has_the_pools_origins() {
    let pool = crate::HttpPool::builder()
        .build()
        .expect("the defaults build");
    let with = ClientBuilder::new("http://agent.example").with_pool(&pool);
    let origins = with.pool_origins().expect("a pool has origins");
    assert!(Arc::ptr_eq(&origins, pool.origin_registry()));
    assert!(
        ClientBuilder::new("http://agent.example")
            .pool_origins()
            .is_none()
    );
}
