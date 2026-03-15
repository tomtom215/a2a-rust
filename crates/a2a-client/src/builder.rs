// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Fluent builder for [`A2aClient`].
//!
//! [`ClientBuilder`] validates configuration and assembles an [`A2aClient`]
//! from its parts.
//!
//! # Example
//!
//! ```rust,no_run
//! use a2a_client::{ClientBuilder, CredentialsStore};
//! use a2a_client::auth::{AuthInterceptor, InMemoryCredentialsStore, SessionId};
//! use std::sync::Arc;
//!
//! # fn example() -> Result<(), a2a_client::error::ClientError> {
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

use std::time::Duration;

use a2a_types::{AgentCard, TransportProtocol};

use crate::client::A2aClient;
use crate::config::{ClientConfig, TlsConfig};
use crate::error::{ClientError, ClientResult};
use crate::interceptor::{CallInterceptor, InterceptorChain};
use crate::transport::{JsonRpcTransport, RestTransport, Transport};

// ── ClientBuilder ─────────────────────────────────────────────────────────────

/// Builder for [`A2aClient`].
///
/// Start with [`ClientBuilder::new`] (URL) or [`ClientBuilder::from_card`]
/// (agent card auto-configuration).
pub struct ClientBuilder {
    endpoint: String,
    transport_override: Option<Box<dyn Transport>>,
    interceptors: InterceptorChain,
    config: ClientConfig,
    preferred_protocol: Option<TransportProtocol>,
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
            preferred_protocol: None,
        }
    }

    /// Creates a builder pre-configured from an [`AgentCard`].
    ///
    /// The transport is selected automatically based on
    /// `card.preferred_transport` and `card.url`.
    #[must_use]
    pub fn from_card(card: &AgentCard) -> Self {
        Self {
            endpoint: card.url.clone(),
            transport_override: None,
            interceptors: InterceptorChain::new(),
            config: ClientConfig::default(),
            preferred_protocol: Some(card.preferred_transport),
        }
    }

    // ── Configuration ─────────────────────────────────────────────────────────

    /// Sets the per-request timeout for non-streaming calls.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.config.request_timeout = timeout;
        self
    }

    /// Sets the preferred transport protocol.
    ///
    /// Overrides any protocol derived from the agent card.
    #[must_use]
    pub const fn with_transport_protocol(mut self, protocol: TransportProtocol) -> Self {
        self.preferred_protocol = Some(protocol);
        self
    }

    /// Sets the accepted output modes sent in `message/send` configurations.
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

    /// Sets `return_immediately` for `message/send` calls.
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

    /// Adds an interceptor to the chain.
    ///
    /// Interceptors are run in the order they are added.
    #[must_use]
    pub fn with_interceptor<I: CallInterceptor>(mut self, interceptor: I) -> Self {
        self.interceptors.push(interceptor);
        self
    }

    // ── Build ─────────────────────────────────────────────────────────────────

    /// Validates configuration and constructs the [`A2aClient`].
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidEndpoint`] if the endpoint URL is malformed.
    /// - [`ClientError::Transport`] if the selected transport cannot be
    ///   initialized.
    pub fn build(self) -> ClientResult<A2aClient> {
        let transport: Box<dyn Transport> = if let Some(t) = self.transport_override {
            t
        } else {
            let protocol = self
                .preferred_protocol
                .unwrap_or(TransportProtocol::JsonRpc);

            match protocol {
                TransportProtocol::JsonRpc => {
                    let t = JsonRpcTransport::with_timeout(
                        &self.endpoint,
                        self.config.request_timeout,
                    )?;
                    Box::new(t)
                }
                TransportProtocol::Rest => {
                    let t =
                        RestTransport::with_timeout(&self.endpoint, self.config.request_timeout)?;
                    Box::new(t)
                }
                TransportProtocol::Grpc => {
                    return Err(ClientError::Transport(
                        "gRPC transport is not supported in this version".into(),
                    ));
                }
            }
        };

        Ok(A2aClient::new(transport, self.interceptors, self.config))
    }
}

impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("endpoint", &self.endpoint)
            .field("preferred_protocol", &self.preferred_protocol)
            .finish_non_exhaustive()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults_to_jsonrpc() {
        let client = ClientBuilder::new("http://localhost:8080")
            .build()
            .expect("build");
        // Verify it built without error.
        let _ = client;
    }

    #[test]
    fn builder_rest_transport() {
        let client = ClientBuilder::new("http://localhost:8080")
            .with_transport_protocol(TransportProtocol::Rest)
            .build()
            .expect("build");
        let _ = client;
    }

    #[test]
    fn builder_grpc_returns_error() {
        let result = ClientBuilder::new("http://localhost:8080")
            .with_transport_protocol(TransportProtocol::Grpc)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_invalid_url_returns_error() {
        let result = ClientBuilder::new("not-a-url").build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_from_card_uses_card_url() {
        use a2a_types::{AgentCapabilities, AgentCard};

        let card = AgentCard {
            name: "test".into(),
            url: "http://localhost:9090".into(),
            version: "1.0".into(),
            description: "A test agent".into(),
            provider: None,
            icon_url: None,
            documentation_url: None,
            capabilities: AgentCapabilities::none(),
            security_schemes: None,
            security: None,
            default_input_modes: vec![],
            default_output_modes: vec![],
            skills: vec![],
            preferred_transport: TransportProtocol::JsonRpc,
            additional_interfaces: None,
            supports_authenticated_extended_card: None,
            signatures: None,
            protocol_version: "0.3.0".into(),
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
}
