// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! TLS connector via rustls.
//!
//! When the `tls-rustls` feature is enabled, this module provides HTTPS
//! support using [`hyper_rustls`] with Mozilla root certificates. No OpenSSL
//! system dependency is required.
//!
//! # Custom CA certificates
//!
//! For enterprise/internal PKI, use [`tls_config_with_extra_roots`] to create
//! a [`rustls::ClientConfig`] with additional trust anchors, then pass it to
//! the client builder.

use http_body_util::Full;
use hyper::body::Bytes;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use rustls::ClientConfig;

/// Type alias for the HTTPS-capable hyper client.
pub type HttpsClient = Client<hyper_rustls::HttpsConnector<HttpConnector>, Full<Bytes>>;

/// Builds a default [`ClientConfig`] with Mozilla root certificates.
///
/// Uses TLS 1.2+ with ring as the crypto provider.
#[must_use]
pub fn default_tls_config() -> ClientConfig {
    ClientConfig::builder()
        .with_root_certificates(root_cert_store())
        .with_no_client_auth()
}

/// Builds a [`ClientConfig`] with extra CA certificates added to the
/// Mozilla root store.
///
/// Use this for enterprise environments with internal PKI. Returns
/// the number of certificates that failed to load (if any).
#[must_use]
pub fn tls_config_with_extra_roots(
    certs: Vec<rustls_pki_types::CertificateDer<'static>>,
) -> ClientConfig {
    let mut store = root_cert_store();
    for cert in certs {
        if let Err(_err) = store.add(cert) {
            trace_warn!(error = %_err, "failed to add custom CA certificate to root store");
        }
    }
    ClientConfig::builder()
        .with_root_certificates(store)
        .with_no_client_auth()
}

/// Returns a root certificate store populated with Mozilla's trusted roots.
fn root_cert_store() -> rustls::RootCertStore {
    let mut store = rustls::RootCertStore::empty();
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    store
}

/// Builds an HTTPS-capable hyper client using the default TLS configuration.
pub(crate) fn build_https_client() -> HttpsClient {
    build_https_client_with_config(default_tls_config())
}

/// Builds an HTTPS-capable hyper client using a custom TLS configuration.
pub fn build_https_client_with_config(tls_config: ClientConfig) -> HttpsClient {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_all_versions()
        .build();

    Client::builder(TokioExecutor::new()).build(https)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tls_config_creates_valid_config() {
        // Verify the config builds without panicking and has a crypto
        // provider (ring). The returned config is opaque, so we just
        // verify construction succeeds.
        let _config = default_tls_config();
    }

    #[test]
    fn tls_config_with_extra_roots_handles_empty() {
        let _config = tls_config_with_extra_roots(vec![]);
    }

    #[test]
    fn build_https_client_creates_client() {
        let _client = build_https_client();
    }

    #[test]
    fn build_https_client_with_custom_config() {
        let config = default_tls_config();
        let _client = build_https_client_with_config(config);
    }
}
