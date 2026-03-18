// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! `GetExtendedAgentCard` handler — returns the full agent card.

use std::collections::HashMap;
use std::time::Instant;

use a2a_protocol_types::agent_card::AgentCard;

use crate::error::{ServerError, ServerResult};

use super::super::helpers::build_call_context;
use super::super::RequestHandler;

impl RequestHandler {
    /// Handles `GetExtendedAgentCard`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Internal`] if no agent card is configured.
    pub async fn on_get_extended_agent_card(
        &self,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<AgentCard> {
        let start = Instant::now();
        self.metrics.on_request("GetExtendedAgentCard");

        let result: ServerResult<_> = async {
            let call_ctx = build_call_context("GetExtendedAgentCard", headers);
            self.interceptors.run_before(&call_ctx).await?;

            let card = self
                .agent_card
                .clone()
                .ok_or_else(|| ServerError::Internal("no agent card configured".into()))?;

            self.interceptors.run_after(&call_ctx).await?;
            Ok(card)
        }
        .await;

        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response("GetExtendedAgentCard");
                self.metrics.on_latency("GetExtendedAgentCard", elapsed);
            }
            Err(e) => {
                self.metrics
                    .on_error("GetExtendedAgentCard", &e.to_string());
                self.metrics.on_latency("GetExtendedAgentCard", elapsed);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface};

    use crate::agent_executor;
    use crate::builder::RequestHandlerBuilder;
    use crate::error::ServerError;

    struct DummyExecutor;
    agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

    fn make_agent_card() -> AgentCard {
        AgentCard {
            url: None,
            name: "Test Agent".into(),
            description: "A test agent".into(),
            version: "1.0.0".into(),
            supported_interfaces: vec![AgentInterface {
                url: "http://localhost:8080".into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            }],
            default_input_modes: vec![],
            default_output_modes: vec![],
            skills: vec![],
            capabilities: AgentCapabilities::none(),
            provider: None,
            icon_url: None,
            documentation_url: None,
            security_schemes: None,
            security_requirements: None,
            signatures: None,
        }
    }

    #[tokio::test]
    async fn get_extended_agent_card_no_card_returns_error() {
        let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
        let result = handler.on_get_extended_agent_card(None).await;
        assert!(
            matches!(result, Err(ServerError::Internal(_))),
            "expected Internal error when no agent card is configured, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn get_extended_agent_card_with_card_returns_ok() {
        let card = make_agent_card();
        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_agent_card(card)
            .build()
            .unwrap();
        let result = handler.on_get_extended_agent_card(None).await;
        assert!(
            result.is_ok(),
            "expected Ok when agent card is configured, got: {result:?}"
        );
        assert_eq!(result.unwrap().name, "Test Agent");
    }

    #[tokio::test]
    async fn get_extended_agent_card_error_path_records_metrics() {
        // Exercises the Err metrics path (line 68) when no agent card is configured.
        let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
        let result = handler.on_get_extended_agent_card(None).await;
        assert!(
            matches!(result, Err(ServerError::Internal(_))),
            "expected Internal error for error metrics path, got: {result:?}"
        );
    }
}
