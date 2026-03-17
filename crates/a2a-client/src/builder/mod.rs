// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Fluent builder for [`A2aClient`](crate::A2aClient).
//!
//! # Module structure
//!
//! | Module | Responsibility |
//! |---|---|
//! | (this file) | Builder struct, configuration setters, card-based construction |
//! | `transport_factory` | `build()` / `build_grpc()` — transport assembly and validation |
//!
//! # Example
//!
//! ```rust,no_run
//! use a2a_protocol_client::{ClientBuilder, CredentialsStore};
//! use a2a_protocol_client::auth::{AuthInterceptor, InMemoryCredentialsStore, SessionId};
//! use std::sync::Arc;
//!
//! # fn example() -> Result<(), a2a_protocol_client::error::ClientError> {
//! let store = Arc::new(InMemoryCredentialsStore::new());
//! let session = SessionId::new("my-session");
//! store.set(session.clone(), "bearer", "token".into());
//!
//! let client = ClientBuilder::new("http://localhost:8080")
//!     .with_interceptor(AuthInterceptor::new(store, session))
//!     .build()?;
//! # Ok(())
//! # }
//! ```

mod transport_factory;

use std::time::Duration;

use a2a_protocol_types::AgentCard;

use crate::config::{ClientConfig, TlsConfig};
use crate::interceptor::{CallInterceptor, InterceptorChain};
use crate::retry::RetryPolicy;
use crate::transport::Transport;

/// The major protocol version supported by this client.
///
/// Used to warn when an agent card advertises an incompatible version.
#[cfg(feature = "tracing")]
const SUPPORTED_PROTOCOL_MAJOR: u32 = 1;

// ── ClientBuilder ─────────────────────────────────────────────────────────────

/// Builder for [`A2aClient`](crate::client::A2aClient).
///
/// Start with [`ClientBuilder::new`] (URL) or [`ClientBuilder::from_card`]
/// (agent card auto-configuration).
pub struct ClientBuilder {
    pub(super) endpoint: String,
    pub(super) transport_override: Option<Box<dyn Transport>>,
    pub(super) interceptors: InterceptorChain,
    pub(super) config: ClientConfig,
    pub(super) preferred_binding: Option<String>,
    pub(super) retry_policy: Option<RetryPolicy>,
}

impl ClientBuilder {
    /// Creates a builder targeting `endpoint`.
    ///
    /// The endpoint is passed directly to the selected transport; it should be
    /// the full base URL of the agent (e.g. `http://localhost:8080`).
    #[must_use]
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            transport_override: None,
            interceptors: InterceptorChain::new(),
            config: ClientConfig::default(),
            preferred_binding: None,
            retry_policy: None,
        }
    }

    /// Creates a builder pre-configured from an [`AgentCard`].
    ///
    /// Selects the first supported interface from the card. Logs a warning
    /// (via `tracing`, if enabled) if the agent's protocol version is not
    /// in the supported range.
    #[must_use]
    pub fn from_card(card: &AgentCard) -> Self {
        let (endpoint, binding) = card
            .supported_interfaces
            .first()
            .map(|i| (i.url.clone(), i.protocol_binding.clone()))
            .unwrap_or_default();

        // Warn if agent advertises a different major version than we support.
        #[cfg(feature = "tracing")]
        if let Some(version) = card
            .supported_interfaces
            .first()
            .map(|i| i.protocol_version.clone())
            .filter(|v| !v.is_empty())
        {
            let major = version
                .split('.')
                .next()
                .and_then(|s| s.parse::<u32>().ok());
            if major != Some(SUPPORTED_PROTOCOL_MAJOR) {
                trace_warn!(
                    agent = %card.name,
                    protocol_version = %version,
                    supported_major = SUPPORTED_PROTOCOL_MAJOR,
                    "agent protocol version may be incompatible with this client"
                );
            }
        }

        Self {
            endpoint,
            transport_override: None,
            interceptors: InterceptorChain::new(),
            config: ClientConfig::default(),
            preferred_binding: Some(binding),
            retry_policy: None,
        }
    }

    // ── Configuration ─────────────────────────────────────────────────────────

    /// Sets the per-request timeout for non-streaming calls.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.config.request_timeout = timeout;
        self
    }

    /// Sets the timeout for establishing SSE stream connections.
    ///
    /// Once the stream is established, this timeout no longer applies.
    /// Defaults to 30 seconds.
    #[must_use]
    pub const fn with_stream_connect_timeout(mut self, timeout: Duration) -> Self {
        self.config.stream_connect_timeout = timeout;
        self
    }

    /// Sets the TCP connection timeout (DNS + handshake).
    ///
    /// Defaults to 10 seconds. Prevents hanging for the OS default (~2 min)
    /// when the server is unreachable.
    #[must_use]
    pub const fn with_connection_timeout(mut self, timeout: Duration) -> Self {
        self.config.connection_timeout = timeout;
        self
    }

    /// Sets the preferred protocol binding.
    ///
    /// Overrides any binding derived from the agent card.
    #[must_use]
    pub fn with_protocol_binding(mut self, binding: impl Into<String>) -> Self {
        self.preferred_binding = Some(binding.into());
        self
    }

    /// Sets the accepted output modes sent in `SendMessage` configurations.
    #[must_use]
    pub fn with_accepted_output_modes(mut self, modes: Vec<String>) -> Self {
        self.config.accepted_output_modes = modes;
        self
    }

    /// Sets the history length to request in task responses.
    #[must_use]
    pub const fn with_history_length(mut self, length: u32) -> Self {
        self.config.history_length = Some(length);
        self
    }

    /// Sets `return_immediately` for `SendMessage` calls.
    #[must_use]
    pub const fn with_return_immediately(mut self, val: bool) -> Self {
        self.config.return_immediately = val;
        self
    }

    /// Provides a fully custom transport implementation.
    ///
    /// Overrides the transport that would normally be built from the endpoint
    /// URL and protocol preference.
    #[must_use]
    pub fn with_custom_transport(mut self, transport: impl Transport) -> Self {
        self.transport_override = Some(Box::new(transport));
        self
    }

    /// Disables TLS (plain HTTP only).
    #[must_use]
    pub const fn without_tls(mut self) -> Self {
        self.config.tls = TlsConfig::Disabled;
        self
    }

    /// Sets a retry policy for transient failures.
    ///
    /// When set, the client automatically retries requests that fail with
    /// transient errors (connection errors, timeouts, HTTP 429/502/503/504)
    /// using exponential backoff.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use a2a_protocol_client::{ClientBuilder, RetryPolicy};
    ///
    /// # fn example() -> Result<(), a2a_protocol_client::error::ClientError> {
    /// let client = ClientBuilder::new("http://localhost:8080")
    ///     .with_retry_policy(RetryPolicy::default())
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = Some(policy);
        self
    }

    /// Adds an interceptor to the chain.
    ///
    /// Interceptors are run in the order they are added.
    #[must_use]
    pub fn with_interceptor<I: CallInterceptor>(mut self, interceptor: I) -> Self {
        self.interceptors.push(interceptor);
        self
    }
}

impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("endpoint", &self.endpoint)
            .field("preferred_binding", &self.preferred_binding)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn builder_from_card_uses_card_url() {
        use a2a_protocol_types::{AgentCapabilities, AgentCard, AgentInterface};

        let card = AgentCard {
            name: "test".into(),
            version: "1.0".into(),
            description: "A test agent".into(),
            supported_interfaces: vec![AgentInterface {
                url: "http://localhost:9090".into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            }],
            provider: None,
            icon_url: None,
            documentation_url: None,
            capabilities: AgentCapabilities::none(),
            security_schemes: None,
            security_requirements: None,
            default_input_modes: vec![],
            default_output_modes: vec![],
            skills: vec![],
            signatures: None,
        };

        let client = ClientBuilder::from_card(&card).build().expect("build");
        let _ = client;
    }

    #[test]
    fn builder_with_timeout_sets_config() {
        let client = ClientBuilder::new("http://localhost:8080")
            .with_timeout(Duration::from_secs(60))
            .build()
            .expect("build");
        assert_eq!(client.config().request_timeout, Duration::from_secs(60));
    }

    #[test]
    fn builder_from_card_empty_interfaces_uses_defaults() {
        use a2a_protocol_types::{AgentCapabilities, AgentCard};

        let card = AgentCard {
            name: "empty".into(),
            version: "1.0".into(),
            description: "No interfaces".into(),
            supported_interfaces: vec![],
            provider: None,
            icon_url: None,
            documentation_url: None,
            capabilities: AgentCapabilities::none(),
            security_schemes: None,
            security_requirements: None,
            default_input_modes: vec![],
            default_output_modes: vec![],
            skills: vec![],
            signatures: None,
        };

        let builder = ClientBuilder::from_card(&card);
        let _ = format!("{builder:?}");
    }

    #[test]
    fn builder_with_return_immediately() {
        let client = ClientBuilder::new("http://localhost:8080")
            .with_return_immediately(true)
            .build()
            .expect("build");
        assert!(client.config().return_immediately);
    }

    #[test]
    fn builder_with_history_length() {
        let client = ClientBuilder::new("http://localhost:8080")
            .with_history_length(10)
            .build()
            .expect("build");
        assert_eq!(client.config().history_length, Some(10));
    }

    #[test]
    fn builder_debug_contains_fields() {
        let builder = ClientBuilder::new("http://localhost:8080");
        let debug = format!("{builder:?}");
        assert!(
            debug.contains("ClientBuilder"),
            "debug output missing struct name: {debug}"
        );
        assert!(
            debug.contains("http://localhost:8080"),
            "debug output missing endpoint: {debug}"
        );
    }

    /// Covers line 107 (version mismatch warning branch in from_card with tracing).
    /// Even without tracing feature, this exercises the code path.
    #[test]
    fn builder_from_card_mismatched_version() {
        use a2a_protocol_types::{AgentCapabilities, AgentCard, AgentInterface};

        let card = AgentCard {
            name: "mismatch".into(),
            version: "1.0".into(),
            description: "Version mismatch test".into(),
            supported_interfaces: vec![AgentInterface {
                url: "http://localhost:9091".into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: "99.0.0".into(), // non-matching major version
                tenant: None,
            }],
            provider: None,
            icon_url: None,
            documentation_url: None,
            capabilities: AgentCapabilities::none(),
            security_schemes: None,
            security_requirements: None,
            default_input_modes: vec![],
            default_output_modes: vec![],
            skills: vec![],
            signatures: None,
        };

        let builder = ClientBuilder::from_card(&card);
        assert_eq!(builder.endpoint, "http://localhost:9091");
    }

    /// Covers lines 150-153 (with_connection_timeout) and 221-224 (with_retry_policy).
    #[test]
    fn builder_with_connection_timeout_and_retry_policy() {
        use crate::retry::RetryPolicy;

        let client = ClientBuilder::new("http://localhost:8080")
            .with_connection_timeout(Duration::from_secs(5))
            .with_retry_policy(RetryPolicy::default())
            .build()
            .expect("build");
        assert_eq!(client.config().connection_timeout, Duration::from_secs(5));
    }

    /// Covers with_stream_connect_timeout (line ~140).
    #[test]
    fn builder_with_stream_connect_timeout() {
        let client = ClientBuilder::new("http://localhost:8080")
            .with_stream_connect_timeout(Duration::from_secs(15))
            .build()
            .expect("build");
        assert_eq!(
            client.config().stream_connect_timeout,
            Duration::from_secs(15)
        );
    }
}
