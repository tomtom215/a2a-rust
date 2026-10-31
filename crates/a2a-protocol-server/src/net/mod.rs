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
//! - [`roots`]: what is trusted (Mozilla's roots plus `SSL_CERT_FILE` and
//!   `SSL_CERT_DIR`) and the `rustls` configuration built from it;
//! - [`client`]: the pooled client, tuned for the load balancers in front of
//!   a receiver (a 50 s idle pool, a bounded connect, TCP keepalive).

// `client` is the push, JWKS and model client: it needs only `tls-rustls`; OTLP
// builds its own on the connector in `proxy`.
#[cfg(feature = "tls-rustls")]
pub mod client;
pub mod proxy;
#[cfg(feature = "tls-rustls")]
pub mod roots;
