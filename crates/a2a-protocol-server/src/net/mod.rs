// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The one way this crate dials out over HTTP.
//!
//! Push delivery, JWKS and OIDC discovery, the tool loop's model client and
//! OTLP export each built their own client, and only OTLP read
//! `HTTPS_PROXY`, `NO_PROXY` or `SSL_CERT_FILE`: in a network whose only way
//! out is a proxy, or whose certificate authority is private, webhooks, token
//! verification and model calls failed while telemetry flowed (audit N-B-1,
//! found 2026-10-31). They now share:
//!
//! - [`proxy`]: the rules (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`,
//!   `NO_PROXY`) and the connector that tunnels or forwards by them;
//! - [`resolver`]: name resolution, with a way to fix a host to the addresses
//!   that were checked for the length of a delivery (DNS-rebinding defence for
//!   `https://` webhooks);
//! - [`roots`]: what is trusted (Mozilla's roots plus `SSL_CERT_FILE` and
//!   `SSL_CERT_DIR`) and the `rustls` configuration built from it;
//! - [`client`]: the pooled client, tuned for the load balancers in front of
//!   a receiver (a 50 s idle pool, a bounded connect, TCP keepalive).
//!
//! The proxy rules, the resolver and the client are in every build: until
//! 0.16.0 a build without `tls-rustls` had none of them, and its push sender
//! read no proxy variable (audit NEW-b-3).

// `client` is the push, JWKS and model client; OTLP builds its own on the
// connector in `proxy`. Without `tls-rustls` the push client is plaintext and
// dials through `plain` (audit NEW-b-3).
pub mod client;
#[cfg(not(feature = "tls-rustls"))]
pub mod plain;
pub mod proxy;
pub mod resolver;
#[cfg(feature = "tls-rustls")]
pub mod roots;
