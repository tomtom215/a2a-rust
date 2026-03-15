// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Client configuration types.
//!
//! [`ClientConfig`] controls how the client connects to agents: which transport
//! to prefer, what content types to accept, timeouts, and TLS settings.

use std::time::Duration;

// ── ProtocolBinding ─────────────────────────────────────────────────────────

/// Protocol binding identifier.
///
/// In v1.0, protocol bindings are free-form strings (`"JSONRPC"`, `"REST"`,
/// `"GRPC"`) rather than a fixed enum.
pub const BINDING_JSONRPC: &str = "JSONRPC";

/// HTTP+JSON protocol binding (spec name for the REST transport).
pub const BINDING_HTTP_JSON: &str = "HTTP+JSON";

/// REST protocol binding (legacy alias for [`BINDING_HTTP_JSON`]).
pub const BINDING_REST: &str = "REST";

/// gRPC protocol binding.
pub const BINDING_GRPC: &str = "GRPC";

// ── TlsConfig ────────────────────────────────────────────────────────────────

/// TLS configuration for the HTTP client.
///
/// When TLS is disabled, the client only supports plain HTTP (`http://` URLs).
/// Enable the `tls-rustls` feature to support HTTPS.
#[derive(Debug, Clone)]
pub enum TlsConfig {
    /// Plain HTTP only; HTTPS connections will fail.
    Disabled,
    /// Enable TLS using the system's default configuration.
    ///
    /// Requires the `tls-rustls` feature.
    #[cfg(feature = "tls-rustls")]
    Rustls,
}

#[allow(clippy::derivable_impls)]
impl Default for TlsConfig {
    fn default() -> Self {
        #[cfg(feature = "tls-rustls")]
        {
            Self::Rustls
        }
        #[cfg(not(feature = "tls-rustls"))]
        {
            Self::Disabled
        }
    }
}

// ── ClientConfig ──────────────────────────────────────────────────────────────

/// Configuration for an [`crate::A2aClient`] instance.
///
/// Build via [`crate::ClientBuilder`]. Reasonable defaults are provided for all
/// fields; most users only need to set the agent URL.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Ordered list of preferred protocol bindings.
    ///
    /// The client tries each in order, selecting the first one supported by the
    /// target agent's card. Defaults to `["JSONRPC"]`.
    pub preferred_bindings: Vec<String>,

    /// MIME types the client will advertise in `acceptedOutputModes`.
    ///
    /// Defaults to `["text/plain", "application/json"]`.
    pub accepted_output_modes: Vec<String>,

    /// Number of historical messages to include in task responses.
    ///
    /// `None` means use the agent's default.
    pub history_length: Option<u32>,

    /// If `true`, `send_message` returns immediately with the submitted task
    /// rather than waiting for completion.
    pub return_immediately: bool,

    /// Per-request timeout for non-streaming calls.
    ///
    /// Defaults to 30 seconds.
    pub request_timeout: Duration,

    /// Per-request timeout for establishing the SSE stream.
    ///
    /// Once the stream is established this timeout no longer applies.
    /// Defaults to 30 seconds.
    pub stream_connect_timeout: Duration,

    /// TLS configuration.
    pub tls: TlsConfig,
}

impl ClientConfig {
    /// Returns the default configuration suitable for connecting to a local
    /// or well-known agent over plain HTTP.
    #[must_use]
    pub fn default_http() -> Self {
        Self {
            preferred_bindings: vec![BINDING_JSONRPC.into()],
            accepted_output_modes: vec!["text/plain".into(), "application/json".into()],
            history_length: None,
            return_immediately: false,
            request_timeout: Duration::from_secs(30),
            stream_connect_timeout: Duration::from_secs(30),
            tls: TlsConfig::Disabled,
        }
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            preferred_bindings: vec![BINDING_JSONRPC.into()],
            accepted_output_modes: vec!["text/plain".into(), "application/json".into()],
            history_length: None,
            return_immediately: false,
            request_timeout: Duration::from_secs(30),
            stream_connect_timeout: Duration::from_secs(30),
            tls: TlsConfig::default(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_jsonrpc_binding() {
        let cfg = ClientConfig::default();
        assert_eq!(cfg.preferred_bindings, vec![BINDING_JSONRPC]);
    }

    #[test]
    fn default_config_timeout() {
        let cfg = ClientConfig::default();
        assert_eq!(cfg.request_timeout, Duration::from_secs(30));
    }

    #[test]
    fn default_http_config_is_disabled_tls() {
        let cfg = ClientConfig::default_http();
        assert!(matches!(cfg.tls, TlsConfig::Disabled));
    }
}
