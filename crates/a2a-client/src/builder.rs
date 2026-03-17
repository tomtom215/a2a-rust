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

use std::time::Duration;

use a2a_protocol_types::AgentCard;

use crate::client::A2aClient;
use crate::config::{ClientConfig, TlsConfig, BINDING_GRPC, BINDING_JSONRPC, BINDING_REST};
use crate::error::{ClientError, ClientResult};
use crate::interceptor::{CallInterceptor, InterceptorChain};
use crate::retry::{RetryPolicy, RetryTransport};
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
    retry_policy: Option<RetryPolicy>,
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

    // ── Build ─────────────────────────────────────────────────────────────────

    /// Validates configuration and constructs the [`A2aClient`].
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidEndpoint`] if the endpoint URL is malformed.
    /// - [`ClientError::Transport`] if the selected transport cannot be
    ///   initialized.
    #[allow(clippy::too_many_lines)]
    pub fn build(self) -> ClientResult<A2aClient> {
        if self.config.request_timeout.is_zero() {
            return Err(ClientError::Transport(
                "request_timeout must be non-zero".into(),
            ));
        }
        if self.config.stream_connect_timeout.is_zero() {
            return Err(ClientError::Transport(
                "stream_connect_timeout must be non-zero".into(),
            ));
        }

        let transport: Box<dyn Transport> = if let Some(t) = self.transport_override {
            t
        } else {
            let binding = self
                .preferred_binding
                .unwrap_or_else(|| BINDING_JSONRPC.into());

            match binding.as_str() {
                BINDING_JSONRPC => {
                    let t = JsonRpcTransport::with_timeouts(
                        &self.endpoint,
                        self.config.request_timeout,
                        self.config.stream_connect_timeout,
                    )?;
                    Box::new(t)
                }
                BINDING_REST => {
                    let t = RestTransport::with_timeouts(
                        &self.endpoint,
                        self.config.request_timeout,
                        self.config.stream_connect_timeout,
                    )?;
                    Box::new(t)
                }
                #[cfg(feature = "grpc")]
                BINDING_GRPC => {
                    // gRPC transport requires async connect; can't do in
                    // sync build(). Use with_custom_transport() instead,
                    // or use ClientBuilder::build_async().
                    return Err(ClientError::Transport(
                        "gRPC transport requires async connect; \
                         use ClientBuilder::build_grpc() or \
                         with_custom_transport(GrpcTransport::connect(...))"
                            .into(),
                    ));
                }
                #[cfg(not(feature = "grpc"))]
                BINDING_GRPC => {
                    return Err(ClientError::Transport(
                        "gRPC transport requires the `grpc` feature flag".into(),
                    ));
                }
                other => {
                    return Err(ClientError::Transport(format!(
                        "unknown protocol binding: {other}"
                    )));
                }
            }
        };

        // Wrap with retry transport if a policy is configured.
        let transport: Box<dyn Transport> = if let Some(policy) = self.retry_policy {
            Box::new(RetryTransport::new(transport, policy))
        } else {
            transport
        };

        Ok(A2aClient::new(transport, self.interceptors, self.config))
    }

    /// Validates configuration and constructs a gRPC-backed [`A2aClient`].
    ///
    /// Unlike [`build`](Self::build), this method is async because gRPC
    /// transport requires establishing a connection.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidEndpoint`] if the endpoint URL is malformed.
    /// - [`ClientError::Transport`] if the gRPC connection fails.
    #[cfg(feature = "grpc")]
    pub async fn build_grpc(self) -> ClientResult<A2aClient> {
        use crate::transport::grpc::{GrpcTransport, GrpcTransportConfig};

        if self.config.request_timeout.is_zero() {
            return Err(ClientError::Transport(
                "request_timeout must be non-zero".into(),
            ));
        }

        let transport: Box<dyn Transport> = if let Some(t) = self.transport_override {
            t
        } else {
            let grpc_config = GrpcTransportConfig::default()
                .with_timeout(self.config.request_timeout)
                .with_connect_timeout(self.config.connection_timeout);
            let t = GrpcTransport::connect_with_config(&self.endpoint, grpc_config).await?;
            Box::new(t)
        };

        let transport: Box<dyn Transport> = if let Some(policy) = self.retry_policy {
            Box::new(RetryTransport::new(transport, policy))
        } else {
            transport
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
    fn builder_grpc_sync_build_returns_error() {
        // gRPC requires async connect; sync build() returns an error
        // directing users to build_grpc().
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
}
