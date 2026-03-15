// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! `agent/authenticatedExtendedCard` client method.

use a2a_types::AuthenticatedExtendedCardResponse;

use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};
use crate::interceptor::{ClientRequest, ClientResponse};

impl A2aClient {
    /// Fetches the full (private) agent card, authenticating the request.
    ///
    /// Calls `agent/authenticatedExtendedCard`. The returned card may include
    /// private skills, security schemes, or additional interfaces not exposed
    /// in the public `/.well-known/agent-card.json`.
    ///
    /// The caller must have registered auth credentials via
    /// [`crate::auth::AuthInterceptor`] or equivalent before calling this
    /// method.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Protocol`] with
    /// [`a2a_types::ErrorCode::AuthenticationFailed`] if the auth credentials
    /// are missing or invalid.
    pub async fn get_authenticated_extended_card(
        &self,
    ) -> ClientResult<AuthenticatedExtendedCardResponse> {
        const METHOD: &str = "agent/authenticatedExtendedCard";

        let mut req = ClientRequest::new(METHOD, serde_json::Value::Null);
        self.interceptors.run_before(&mut req).await?;

        let result = self
            .transport
            .send_request(
                METHOD,
                serde_json::Value::Object(serde_json::Map::new()),
                &req.extra_headers,
            )
            .await?;

        let resp = ClientResponse {
            method: METHOD.to_owned(),
            result: result.clone(),
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<AuthenticatedExtendedCardResponse>(result)
            .map_err(ClientError::Serialization)
    }
}
