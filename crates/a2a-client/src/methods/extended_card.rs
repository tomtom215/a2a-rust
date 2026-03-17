// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! `GetExtendedAgentCard` client method.

use a2a_protocol_types::AuthenticatedExtendedCardResponse;

use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};
use crate::interceptor::{ClientRequest, ClientResponse};

impl A2aClient {
    /// Fetches the full (private) agent card, authenticating the request.
    ///
    /// Calls `GetExtendedAgentCard`. The returned card may include
    /// private skills, security schemes, or additional interfaces not exposed
    /// in the public `/.well-known/agent.json`.
    ///
    /// The caller must have registered auth credentials via
    /// [`crate::auth::AuthInterceptor`] or equivalent before calling this
    /// method.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or protocol errors.
    pub async fn get_extended_agent_card(&self) -> ClientResult<AuthenticatedExtendedCardResponse> {
        const METHOD: &str = "GetExtendedAgentCard";

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
            result,
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<AuthenticatedExtendedCardResponse>(resp.result)
            .map_err(ClientError::Serialization)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;

    use crate::error::{ClientError, ClientResult};
    use crate::streaming::EventStream;
    use crate::transport::Transport;
    use crate::ClientBuilder;

    struct MockTransport {
        response: serde_json::Value,
    }

    impl MockTransport {
        fn new(response: serde_json::Value) -> Self {
            Self { response }
        }
    }

    impl Transport for MockTransport {
        fn send_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
            let resp = self.response.clone();
            Box::pin(async move { Ok(resp) })
        }

        fn send_streaming_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
            Box::pin(async move { Err(ClientError::Transport("not supported".into())) })
        }
    }

    struct ErrorTransport {
        error_msg: String,
    }

    impl Transport for ErrorTransport {
        fn send_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
            let msg = self.error_msg.clone();
            Box::pin(async move { Err(ClientError::Transport(msg)) })
        }

        fn send_streaming_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
            let msg = self.error_msg.clone();
            Box::pin(async move { Err(ClientError::Transport(msg)) })
        }
    }

    fn make_client(transport: impl Transport) -> crate::A2aClient {
        ClientBuilder::new("http://localhost:8080")
            .with_custom_transport(transport)
            .build()
            .expect("build client")
    }

    fn agent_card_json() -> serde_json::Value {
        serde_json::json!({
            "name": "test-agent",
            "description": "A test agent",
            "version": "1.0.0",
            "supportedInterfaces": [{
                "url": "http://localhost:8080",
                "protocolBinding": "JSONRPC",
                "protocolVersion": "1.0.0"
            }],
            "defaultInputModes": ["text/plain"],
            "defaultOutputModes": ["text/plain"],
            "skills": [{
                "id": "echo",
                "name": "Echo",
                "description": "Echoes input",
                "tags": ["test"]
            }],
            "capabilities": {}
        })
    }

    #[tokio::test]
    async fn get_extended_agent_card_success() {
        let transport = MockTransport::new(agent_card_json());
        let client = make_client(transport);

        let card = client.get_extended_agent_card().await.unwrap();
        assert_eq!(card.name, "test-agent");
        assert_eq!(card.version, "1.0.0");
        assert_eq!(card.skills.len(), 1);
        assert_eq!(card.skills[0].id, "echo");
    }

    #[tokio::test]
    async fn get_extended_agent_card_transport_error() {
        let transport = ErrorTransport {
            error_msg: "connection refused".into(),
        };
        let client = make_client(transport);

        let err = client.get_extended_agent_card().await.unwrap_err();
        assert!(
            matches!(err, ClientError::Transport(ref msg) if msg.contains("connection refused")),
            "expected Transport error, got {err:?}"
        );
    }
}
