// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Health Monitor executor — checks agent health via A2A client calls.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::task::TaskState;

use a2a_protocol_client::{resolve_agent_card, ClientBuilder};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::executor_helpers::boxed_future;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

use crate::helpers::EventEmitter;

/// Checks agent health by querying their agent cards and task lists. Reports
/// a summary artifact. Designed to test push notification delivery.
pub struct HealthMonitorExecutor;

impl AgentExecutor for HealthMonitorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        boxed_future(async move {
            let emit = EventEmitter::new(ctx, queue);

            emit.status(TaskState::Working).await?;

            // The message should contain agent URLs as JSON data part.
            let agent_urls: Vec<String> = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Data { data } => {
                        serde_json::from_value::<Vec<String>>(data.clone()).ok()
                    }
                    _ => None,
                })
                .unwrap_or_default();

            let mut results = Vec::new();

            for url in &agent_urls {
                let status = check_agent_health(url).await;
                results.push(format!("{url}: {status}"));
            }

            // Also check via text part (for simple "ping" messages).
            let text_input = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default();

            if !text_input.is_empty() {
                results.push(format!("Received check request: {text_input}"));
            }

            let report = if results.is_empty() {
                "No agents to check — health monitor standing by.".to_owned()
            } else {
                format!("Health Report:\n{}", results.join("\n"))
            };

            emit.artifact("health-report", vec![Part::text(&report)], None, Some(true))
                .await?;

            emit.status(TaskState::Completed).await?;

            Ok(())
        })
    }
}

/// Checks an individual agent's health by resolving its card and listing tasks.
async fn check_agent_health(url: &str) -> &'static str {
    let list_params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: Some(1),
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };

    match resolve_agent_card(url).await {
        Ok(card) => {
            let binding = card
                .supported_interfaces
                .first()
                .map(|i| i.protocol_binding.as_str())
                .unwrap_or("JSONRPC");
            match ClientBuilder::new(url)
                .with_protocol_binding(binding)
                .build()
            {
                Ok(client) => match client.list_tasks(list_params).await {
                    Ok(_) => "HEALTHY",
                    Err(_) => "DEGRADED",
                },
                Err(_) => "UNREACHABLE",
            }
        }
        Err(_) => {
            // Fall back to default JSON-RPC if agent card is unavailable.
            match ClientBuilder::new(url).build() {
                Ok(client) => match client.list_tasks(list_params).await {
                    Ok(_) => "HEALTHY",
                    Err(_) => "DEGRADED",
                },
                Err(_) => "UNREACHABLE",
            }
        }
    }
}
