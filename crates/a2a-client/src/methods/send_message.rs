// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! `SendMessage` and `SendStreamingMessage` client methods.

use a2a_protocol_types::params::SendMessageConfiguration;
use a2a_protocol_types::{MessageSendParams, SendMessageResponse};

use crate::client::A2aClient;
use crate::config::ClientConfig;
use crate::error::{ClientError, ClientResult};
use crate::interceptor::{ClientRequest, ClientResponse};
use crate::streaming::EventStream;

/// Merge client-level config into `MessageSendParams.configuration`.
///
/// Per-request values in `params.configuration` take precedence over
/// client-level defaults so callers can override on a per-call basis.
fn apply_client_config(params: &mut MessageSendParams, config: &ClientConfig) {
    let cfg = params
        .configuration
        .get_or_insert_with(SendMessageConfiguration::default);

    // Only apply client-level settings when the per-request value is absent.
    if cfg.return_immediately.is_none() && config.return_immediately {
        cfg.return_immediately = Some(true);
    }
    if cfg.history_length.is_none() {
        if let Some(hl) = config.history_length {
            cfg.history_length = Some(hl);
        }
    }
    if cfg.accepted_output_modes.is_empty() && !config.accepted_output_modes.is_empty() {
        cfg.accepted_output_modes.clone_from(&config.accepted_output_modes);
    }
}

impl A2aClient {
    /// Sends a message to the agent and waits for a complete response.
    ///
    /// Calls the `SendMessage` JSON-RPC method. The agent may respond with
    /// either a completed [`Task`] or an immediate [`Message`].
    ///
    /// Client-level configuration (e.g. `return_immediately`, `history_length`)
    /// is automatically merged into the request parameters. Per-request values
    /// take precedence.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport, serialization, or protocol errors.
    ///
    /// [`Task`]: a2a_protocol_types::Task
    /// [`Message`]: a2a_protocol_types::Message
    pub async fn send_message(
        &self,
        mut params: MessageSendParams,
    ) -> ClientResult<SendMessageResponse> {
        const METHOD: &str = "SendMessage";

        apply_client_config(&mut params, &self.config);

        let params_value = serde_json::to_value(&params).map_err(ClientError::Serialization)?;

        let mut req = ClientRequest::new(METHOD, params_value);
        self.interceptors.run_before(&mut req).await?;

        let result = self
            .transport
            .send_request(METHOD, req.params, &req.extra_headers)
            .await?;

        let resp = ClientResponse {
            method: METHOD.to_owned(),
            result,
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<SendMessageResponse>(resp.result)
            .map_err(ClientError::Serialization)
    }

    /// Sends a message and returns a streaming [`EventStream`] of progress
    /// events.
    ///
    /// Calls the `SendStreamingMessage` JSON-RPC method. The agent responds
    /// with an SSE stream of [`a2a_protocol_types::StreamResponse`] events.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or protocol errors.
    pub async fn stream_message(
        &self,
        mut params: MessageSendParams,
    ) -> ClientResult<EventStream> {
        const METHOD: &str = "SendStreamingMessage";

        apply_client_config(&mut params, &self.config);

        let params_value = serde_json::to_value(&params).map_err(ClientError::Serialization)?;

        let mut req = ClientRequest::new(METHOD, params_value);
        self.interceptors.run_before(&mut req).await?;

        self.transport
            .send_streaming_request(METHOD, req.params, &req.extra_headers)
            .await
    }
}
