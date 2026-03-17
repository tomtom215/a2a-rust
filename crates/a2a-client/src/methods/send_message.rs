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
        cfg.accepted_output_modes
            .clone_from(&config.accepted_output_modes);
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
    pub async fn stream_message(&self, mut params: MessageSendParams) -> ClientResult<EventStream> {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::{Message, MessageId, MessageRole, Part};

    fn make_params() -> MessageSendParams {
        MessageSendParams {
            tenant: None,
            message: Message {
                id: MessageId::new("msg-1"),
                role: MessageRole::User,
                parts: vec![Part::text("test")],
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            },
            configuration: None,
            metadata: None,
        }
    }

    #[test]
    fn apply_config_sets_return_immediately_when_absent() {
        let config = ClientConfig {
            return_immediately: true,
            ..ClientConfig::default()
        };

        let mut params = make_params();
        apply_client_config(&mut params, &config);

        let cfg = params.configuration.unwrap();
        assert_eq!(cfg.return_immediately, Some(true));
    }

    #[test]
    fn apply_config_does_not_override_per_request_return_immediately() {
        let config = ClientConfig {
            return_immediately: true,
            ..ClientConfig::default()
        };

        let mut params = make_params();
        params.configuration = Some(SendMessageConfiguration {
            return_immediately: Some(false),
            ..Default::default()
        });
        apply_client_config(&mut params, &config);

        let cfg = params.configuration.unwrap();
        assert_eq!(
            cfg.return_immediately,
            Some(false),
            "per-request value should take precedence"
        );
    }

    #[test]
    fn apply_config_does_not_set_return_immediately_when_config_false() {
        let config = ClientConfig::default(); // return_immediately = false

        let mut params = make_params();
        apply_client_config(&mut params, &config);

        let cfg = params.configuration.unwrap();
        assert_eq!(
            cfg.return_immediately, None,
            "should not set return_immediately when config is false"
        );
    }

    #[test]
    fn apply_config_sets_history_length_when_absent() {
        let config = ClientConfig {
            history_length: Some(10),
            ..ClientConfig::default()
        };

        let mut params = make_params();
        apply_client_config(&mut params, &config);

        let cfg = params.configuration.unwrap();
        assert_eq!(cfg.history_length, Some(10));
    }

    #[test]
    fn apply_config_does_not_override_per_request_history_length() {
        let config = ClientConfig {
            history_length: Some(10),
            ..ClientConfig::default()
        };

        let mut params = make_params();
        params.configuration = Some(SendMessageConfiguration {
            history_length: Some(5),
            ..Default::default()
        });
        apply_client_config(&mut params, &config);

        let cfg = params.configuration.unwrap();
        assert_eq!(cfg.history_length, Some(5));
    }

    #[test]
    fn apply_config_sets_accepted_output_modes_when_empty() {
        let config = ClientConfig {
            accepted_output_modes: vec!["audio/wav".into()],
            ..ClientConfig::default()
        };

        let mut params = make_params();
        // Explicitly set empty modes so the config value is applied.
        // (SendMessageConfiguration::default() has non-empty modes, so we must override.)
        params.configuration = Some(SendMessageConfiguration {
            accepted_output_modes: vec![],
            task_push_notification_config: None,
            history_length: None,
            return_immediately: None,
        });
        apply_client_config(&mut params, &config);

        let cfg = params.configuration.unwrap();
        assert_eq!(cfg.accepted_output_modes, vec!["audio/wav"]);
    }

    #[test]
    fn apply_config_does_not_override_per_request_output_modes() {
        let config = ClientConfig {
            accepted_output_modes: vec!["text/plain".into()],
            ..ClientConfig::default()
        };

        let mut params = make_params();
        params.configuration = Some(SendMessageConfiguration {
            accepted_output_modes: vec!["application/json".into()],
            ..Default::default()
        });
        apply_client_config(&mut params, &config);

        let cfg = params.configuration.unwrap();
        assert_eq!(cfg.accepted_output_modes, vec!["application/json"]);
    }

    #[test]
    fn apply_config_no_op_when_config_has_no_overrides() {
        let config = ClientConfig::default();
        // Default config: return_immediately=false, history_length=None,
        // accepted_output_modes=["text/plain", "application/json"]

        let mut params = make_params();
        // Pre-set configuration to empty.
        params.configuration = Some(SendMessageConfiguration::default());
        apply_client_config(&mut params, &config);

        let cfg = params.configuration.unwrap();
        // return_immediately should remain None since config.return_immediately is false.
        assert_eq!(cfg.return_immediately, None);
        // history_length should remain None.
        assert_eq!(cfg.history_length, None);
    }

    #[test]
    fn apply_config_does_not_set_modes_when_config_modes_empty() {
        let config = ClientConfig {
            accepted_output_modes: vec![],
            ..ClientConfig::default()
        };

        let mut params = make_params();
        // Pre-set with empty modes to test that config doesn't override.
        params.configuration = Some(SendMessageConfiguration {
            accepted_output_modes: vec![],
            task_push_notification_config: None,
            history_length: None,
            return_immediately: None,
        });
        apply_client_config(&mut params, &config);

        let cfg = params.configuration.unwrap();
        assert!(
            cfg.accepted_output_modes.is_empty(),
            "should not set modes when config modes are empty"
        );
    }
}
