// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What this crate's outbound TLS trusts.
//!
//! Mozilla's roots (`webpki-roots`) plus, when set, the PEM certificates in
//! the file `SSL_CERT_FILE` names and in the directories `SSL_CERT_DIR` names
//! (each a `:`-separated list on Unix, as `PATH`): the variables curl,
//! OpenSSL and Go read. That is how a container image's CA bundle
//! (`SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt`), a TLS-inspecting
//! proxy's root or a cluster's private CA reaches push delivery, JWKS and OIDC
//! discovery and the model client without code. A file that cannot be read or
//! parsed is reported once and skipped; it never removes Mozilla's roots.
//!
//! The variables are read once per process: a bundle rewritten while the
//! process runs is not seen until it restarts (the client crate does the same).
//!
//! This is a copy of the client crate's `tls.rs` root-store functions
//! (`root_cert_store`, `env_roots`, `read_env_roots`); the two crates share no
//! dependency that could hold one. `scripts/check_net_copies.py` fails when the
//! copies differ.

use std::sync::Arc;

use rustls::ClientConfig;
use rustls::crypto::CryptoProvider;

/// The `ring`-backed provider, passed explicitly so that a process holding
/// more than one crypto provider (`aws-lc-rs` through another crate) does not
/// make `ClientConfig::builder` panic.
pub fn ring_provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A config with `store` as its roots and no client certificate.
///
/// # Panics
///
/// Only if the `ring` provider does not support rustls's default protocol
/// versions (TLS 1.2 and 1.3), which it does for every `ring` this crate is
/// pinned against: the construct cannot fail. It replaces three copies of the
/// same `expect` (push delivery, the JWKS client and this module's own).
pub fn config_with_roots(store: rustls::RootCertStore) -> ClientConfig {
    ClientConfig::builder_with_provider(ring_provider())
        .with_safe_default_protocol_versions()
        .expect("ring provider supports the rustls default protocol versions")
        .with_root_certificates(store)
        .with_no_client_auth()
}

/// Builds a default [`ClientConfig`] with Mozilla root certificates.
///
/// Uses TLS 1.2+ with `ring` as the crypto provider, selected explicitly
/// (see the private `ring_provider` helper for the rationale).
///
/// # Panics
///
/// Panics only if the `ring` crypto provider does not support the rustls
/// default protocol versions (TLS 1.2 and 1.3). Both are supported for
/// every `ring` version this crate is pinned against, so in practice this
/// function is infallible.
#[must_use]
pub fn default_tls_config() -> ClientConfig {
    // Built once per process: the roots are (`SSL_CERT_FILE` is read once),
    // and the card, JWKS and token fetches each asked for a fresh config,
    // about 150 certificates extended into a new store, per call (audit
    // N-B-6). A `ClientConfig` clones by reference counting.
    static DEFAULT: std::sync::LazyLock<ClientConfig> =
        std::sync::LazyLock::new(|| config_with_roots(root_cert_store()));
    DEFAULT.clone()
}

/// Mozilla's roots, plus those the environment names (module docs).
pub fn root_cert_store() -> rustls::RootCertStore {
    let mut store = rustls::RootCertStore::empty();
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for cert in env_roots() {
        // Each was parsed once; a duplicate of a Mozilla root is harmless.
        let _ = store.add(cert.clone());
    }
    store
}

/// The certificates `SSL_CERT_FILE` and `SSL_CERT_DIR` name, read once per
/// process.
fn env_roots() -> &'static [rustls_pki_types::CertificateDer<'static>] {
    static ROOTS: std::sync::OnceLock<Vec<rustls_pki_types::CertificateDer<'static>>> =
        std::sync::OnceLock::new();
    ROOTS.get_or_init(|| {
        read_env_roots(
            std::env::var_os("SSL_CERT_FILE").as_deref(),
            std::env::var_os("SSL_CERT_DIR").as_deref(),
        )
    })
}

/// The certificates in each file of `files` and in each directory of `dirs`
/// (both `:`-separated on Unix, `;` on Windows, as `PATH`).
pub fn read_env_roots(
    files: Option<&std::ffi::OsStr>,
    dirs: Option<&std::ffi::OsStr>,
) -> Vec<rustls_pki_types::CertificateDer<'static>> {
    use rustls_pki_types::CertificateDer;
    use rustls_pki_types::pem::PemObject;

    let mut paths: Vec<std::path::PathBuf> = files
        .map(|f| std::env::split_paths(f).collect())
        .unwrap_or_default();
    for dir in dirs
        .map(|d| std::env::split_paths(d).collect::<Vec<_>>())
        .unwrap_or_default()
    {
        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                let mut found: Vec<_> = entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .collect();
                found.sort();
                paths.extend(found);
            }
            Err(_e) => {
                trace_warn!(dir = %dir.display(), error = %_e, "SSL_CERT_DIR: cannot read; skipped");
            }
        }
    }
    let mut certs = Vec::new();
    for path in paths.iter().filter(|p| !p.as_os_str().is_empty()) {
        match std::fs::read(path) {
            Ok(pem) => {
                let before = certs.len();
                // A directory of hashed links holds non-PEM files too; only
                // what parses is kept.
                certs.extend(CertificateDer::pem_slice_iter(&pem).filter_map(Result::ok));
                if certs.len() == before {
                    trace_debug!(file = %path.display(), "SSL_CERT_*: no PEM certificate in file");
                }
            }
            Err(_e) => {
                trace_warn!(file = %path.display(), error = %_e, "SSL_CERT_FILE: cannot read; skipped");
            }
        }
    }
    certs
}

#[cfg(test)]
mod tests;
