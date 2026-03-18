// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Transport assembly and client construction.
//!
//! Contains the `build()` and `build_grpc()` methods that validate
//! configuration, select the appropriate transport, and wire everything
//! together into an [`A2aClient`].

use crate::client::A2aClient;
use crate::config::{BINDING_GRPC, BINDING_JSONRPC, BINDING_REST};
use crate::error::{ClientError, ClientResult};
use crate::retry::RetryTransport;
use crate::transport::{JsonRpcTransport, RestTransport, Transport};

use super::ClientBuilder;

impl ClientBuilder {
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
                    let t = JsonRpcTransport::with_all_timeouts(
                        &self.endpoint,
                        self.config.request_timeout,
                        self.config.stream_connect_timeout,
                        self.config.connection_timeout,
                    )?;
                    Box::new(t)
                }
                BINDING_REST => {
                    let t = RestTransport::with_all_timeouts(
                        &self.endpoint,
                        self.config.request_timeout,
                        self.config.stream_connect_timeout,
                        self.config.connection_timeout,
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

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::config::{BINDING_GRPC, BINDING_REST};
    use std::time::Duration;

    #[test]
    fn builder_defaults_to_jsonrpc() {
        let client = ClientBuilder::new("http://localhost:8080")
            .build()
            .expect("build");
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
    fn builder_zero_request_timeout_errors() {
        let result = ClientBuilder::new("http://localhost:8080")
            .with_timeout(Duration::ZERO)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_zero_stream_timeout_errors() {
        let result = ClientBuilder::new("http://localhost:8080")
            .with_stream_connect_timeout(Duration::ZERO)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_unknown_binding_errors() {
        let result = ClientBuilder::new("http://localhost:8080")
            .with_protocol_binding("UNKNOWN_PROTOCOL")
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_rest_with_retry_policy() {
        use crate::retry::RetryPolicy;

        // Covers lines 60 (REST Box::new) and 91 (retry wrapping).
        let client = ClientBuilder::new("http://localhost:8080")
            .with_protocol_binding(BINDING_REST)
            .with_retry_policy(RetryPolicy::default())
            .build()
            .expect("build");
        let _ = client;
    }

    #[test]
    fn builder_jsonrpc_with_retry_policy() {
        use crate::retry::RetryPolicy;

        // Covers line 91 (retry wrapping with JSONRPC transport).
        let client = ClientBuilder::new("http://localhost:8080")
            .with_retry_policy(RetryPolicy::default())
            .build()
            .expect("build");
        let _ = client;
    }

    #[test]
    fn builder_from_card_rejects_incompatible_binding() {
        use a2a_protocol_types::{AgentCapabilities, AgentCard, AgentInterface};

        let card = AgentCard {
            name: "test".into(),
            version: "1.0".into(),
            description: "Test agent".into(),
            supported_interfaces: vec![AgentInterface {
                url: "http://localhost:9090".into(),
                protocol_binding: "UNKNOWN".into(),
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

        let result = ClientBuilder::from_card(&card).build();
        assert!(result.is_err(), "unknown binding should fail");
    }
}
