// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Core [`A2aClient`] struct and constructors.
//!
//! [`A2aClient`] is the top-level entry point for all A2A protocol operations.
//! Construct one via [`ClientBuilder`] or via the convenience
//! [`A2aClient::from_card`] method.
//!
//! All A2A methods are provided by `impl` blocks in the `methods/` modules:
//!
//! | Module | Methods |
//! |---|---|
//! | [`crate::methods::send_message`] | `send_message`, `stream_message` |
//! | [`crate::methods::tasks`] | `get_task`, `list_tasks`, `cancel_task`, `resubscribe` |
//! | [`crate::methods::push_config`] | `set_push_config`, `get_push_config`, `list_push_configs`, `delete_push_config` |
//! | [`crate::methods::extended_card`] | `get_authenticated_extended_card` |
//!
//! [`ClientBuilder`]: crate::ClientBuilder

use a2a_protocol_types::AgentCard;

use crate::builder::ClientBuilder;
use crate::config::ClientConfig;
use crate::error::ClientResult;
use crate::interceptor::InterceptorChain;
use crate::transport::Transport;

// ── A2aClient ────────────────────────────────────────────────────────────────

/// A client for communicating with A2A-compliant agents.
///
/// All A2A protocol methods are available as `async` methods. Create a client
/// via [`ClientBuilder`] or the [`A2aClient::from_card`] shorthand.
///
/// # Example
///
/// ```rust,no_run
/// use a2a_protocol_client::ClientBuilder;
///
/// # async fn example() -> Result<(), a2a_protocol_client::error::ClientError> {
/// let client = ClientBuilder::new("http://localhost:8080").build()?;
/// # Ok(())
/// # }
/// ```
pub struct A2aClient {
    /// The underlying transport (JSON-RPC or REST).
    pub(crate) transport: Box<dyn Transport>,
    /// Ordered interceptor chain applied to every request/response.
    pub(crate) interceptors: InterceptorChain,
    /// Client configuration.
    pub(crate) config: ClientConfig,
}

impl A2aClient {
    /// Creates a client from an [`AgentCard`] using the recommended defaults.
    ///
    /// Selects the transport based on the agent's preferred protocol.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::ClientError::InvalidEndpoint`] if the agent
    /// card URL is malformed or the transport cannot be constructed.
    pub fn from_card(card: &AgentCard) -> ClientResult<Self> {
        ClientBuilder::from_card(card)?.build()
    }

    /// Returns a reference to the active client configuration.
    #[must_use]
    pub const fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Creates a new [`A2aClient`] from its constituent parts.
    ///
    /// This is the low-level constructor used by [`ClientBuilder`]. Prefer
    /// [`ClientBuilder`] unless you need precise control over each component.
    #[must_use]
    pub(crate) fn new(
        transport: Box<dyn Transport>,
        interceptors: InterceptorChain,
        config: ClientConfig,
    ) -> Self {
        Self {
            transport,
            interceptors,
            config,
        }
    }
}

impl std::fmt::Debug for A2aClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("A2aClient")
            .field("interceptors", &self.interceptors)
            .finish_non_exhaustive()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::JsonRpcTransport;

    #[test]
    fn client_new_stores_config() {
        let transport = JsonRpcTransport::new("http://localhost:8080").expect("transport");
        let client = A2aClient::new(
            Box::new(transport),
            InterceptorChain::new(),
            ClientConfig::default_http(),
        );
        assert_eq!(
            client.config().request_timeout,
            std::time::Duration::from_secs(30)
        );
    }

    #[test]
    fn client_debug_impl() {
        let transport = JsonRpcTransport::new("http://localhost:8080").expect("transport");
        let client = A2aClient::new(
            Box::new(transport),
            InterceptorChain::new(),
            ClientConfig::default_http(),
        );
        let dbg = format!("{client:?}");
        assert!(dbg.contains("A2aClient"));
    }
}
