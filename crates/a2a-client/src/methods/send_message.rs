// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! `message/send` and `message/stream` client methods.

use a2a_types::{MessageSendParams, SendMessageResponse};

use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};
use crate::interceptor::{ClientRequest, ClientResponse};
use crate::streaming::EventStream;

impl A2aClient {
    /// Sends a message to the agent and waits for a complete response.
    ///
    /// Calls the `message/send` JSON-RPC method. The agent may respond with
    /// either a completed [`Task`] or an immediate [`Message`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport, serialization, or protocol errors.
    ///
    /// [`Task`]: a2a_types::Task
    /// [`Message`]: a2a_types::Message
    pub async fn send_message(
        &self,
        params: MessageSendParams,
    ) -> ClientResult<SendMessageResponse> {
        const METHOD: &str = "message/send";

        let params_value = serde_json::to_value(&params).map_err(ClientError::Serialization)?;

        let mut req = ClientRequest::new(METHOD, params_value);
        self.interceptors.run_before(&mut req).await?;

        let result = self
            .transport
            .send_request(METHOD, req.params, &req.extra_headers)
            .await?;

        let resp = ClientResponse {
            method: METHOD.to_owned(),
            result: result.clone(),
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<SendMessageResponse>(result).map_err(ClientError::Serialization)
    }

    /// Sends a message and returns a streaming [`EventStream`] of progress
    /// events.
    ///
    /// Calls the `message/stream` JSON-RPC method. The agent responds with an
    /// SSE stream of [`a2a_types::StreamResponse`] events ending with a
    /// `final: true` [`a2a_types::TaskStatusUpdateEvent`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or protocol errors.
    pub async fn stream_message(&self, params: MessageSendParams) -> ClientResult<EventStream> {
        const METHOD: &str = "message/stream";

        let params_value = serde_json::to_value(&params).map_err(ClientError::Serialization)?;

        let mut req = ClientRequest::new(METHOD, params_value);
        self.interceptors.run_before(&mut req).await?;

        self.transport
            .send_streaming_request(METHOD, req.params, &req.extra_headers)
            .await
    }
}
