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

use a2a_types::AgentCard;

use crate::client::A2aClient;
use crate::config::{ClientConfig, TlsConfig, BINDING_GRPC, BINDING_JSONRPC, BINDING_REST};
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
    preferred_binding: Option<String>,
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
        }
    }

    /// Creates a builder pre-configured from an [`AgentCard`].
    ///
    /// Selects the first supported interface from the card.
    #[must_use]
    pub fn from_card(card: &AgentCard) -> Self {
        let (endpoint, binding) = card
            .supported_interfaces
            .first()
            .map(|i| (i.url.clone(), i.protocol_binding.clone()))
            .unwrap_or_default();

        Self {
            endpoint,
            transport_override: None,
            interceptors: InterceptorChain::new(),
            config: ClientConfig::default(),
            preferred_binding: Some(binding),
        }
    }

    // ── Configuration ─────────────────────────────────────────────────────────

    /// Sets the per-request timeout for non-streaming calls.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.config.request_timeout = timeout;
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
            let binding = self
                .preferred_binding
                .unwrap_or_else(|| BINDING_JSONRPC.into());

            match binding.as_str() {
                BINDING_JSONRPC => {
                    let t = JsonRpcTransport::with_timeout(
                        &self.endpoint,
                        self.config.request_timeout,
                    )?;
                    Box::new(t)
                }
                BINDING_REST => {
                    let t =
                        RestTransport::with_timeout(&self.endpoint, self.config.request_timeout)?;
                    Box::new(t)
                }
                BINDING_GRPC => {
                    return Err(ClientError::Transport(
                        "gRPC transport is not supported in this version".into(),
                    ));
                }
                other => {
                    return Err(ClientError::Transport(format!(
                        "unknown protocol binding: {other}"
                    )));
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
            .field("preferred_binding", &self.preferred_binding)
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
            .with_protocol_binding(BINDING_REST)
            .build()
            .expect("build");
        let _ = client;
    }

    #[test]
    fn builder_grpc_returns_error() {
        let result = ClientBuilder::new("http://localhost:8080")
            .with_protocol_binding(BINDING_GRPC)
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
        use a2a_types::{AgentCapabilities, AgentCard, AgentInterface};

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
            security: None,
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
}
