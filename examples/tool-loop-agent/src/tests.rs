// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The example's wiring and tools with a scripted model, so `cargo test`
//! needs no model: the same served agent and client path the binary takes.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::tool_loop::{
    ANSWER_ARTIFACT_ID, ChatMessage, ChatRequest, ModelError, ModelTurn, TRACE_ARTIFACT_ID,
    ToolCall, ToolModel,
};

use super::{artifact_text, ask, serve_agent};

/// Plays the billing run `examples/rig-agent` recorded: the unknown name,
/// the hint, the inventory, the answer. Checks on its way that the preamble
/// was sent and each result came back under its call's id.
struct Billing(Mutex<VecDeque<ModelTurn>>);

impl Billing {
    fn new() -> Self {
        let call = |id: &str, name: &str, args: &str| ToolCall::new(id, name, args);
        Self(Mutex::new(VecDeque::from([
            ModelTurn::new(
                "",
                vec![call("a", "service_status", r#"{"service":"billing"}"#)],
            ),
            ModelTurn::new("", vec![call("b", "list_services", "{}")]),
            ModelTurn::new(
                "There is no billing service; payments-api is degraded.",
                vec![],
            ),
        ])))
    }
}

impl ToolModel for Billing {
    fn model_name(&self) -> &'static str {
        "scripted"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        assert_eq!(request.preamble, super::PREAMBLE);
        assert_eq!(request.tools.len(), 2);
        let answered: Vec<&str> = request
            .messages
            .iter()
            .filter_map(|m| match m {
                ChatMessage::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
            .collect();
        let next = self.0.lock().unwrap().pop_front();
        let expected: &[&str] = match self.0.lock().unwrap().len() {
            2 => &[],
            1 => &["a"],
            _ => &["a", "b"],
        };
        assert_eq!(answered, expected);
        let reply = next.ok_or_else(|| ModelError::Other("script exhausted".to_owned()));
        Box::pin(async move { reply })
    }
}

#[tokio::test]
async fn the_billing_run_recovers_through_the_real_tools_and_reports_the_trace() {
    let url = serve_agent(Billing::new()).await.unwrap();
    let task = ask(&url, "How is the billing service doing?")
        .await
        .unwrap();
    assert_eq!(task.status.state, TaskState::Completed);
    assert_eq!(
        artifact_text(&task, ANSWER_ARTIFACT_ID),
        Some("There is no billing service; payments-api is degraded.")
    );
    assert_eq!(
        artifact_text(&task, TRACE_ARTIFACT_ID),
        Some(concat!(
            r#"service_status({"service":"billing"}) -> error: no service named 'billing'; call list_services for the inventory"#,
            "\n",
            r#"list_services({}) -> ["payments-api","checkout","search"]"#,
        ))
    );
}

#[tokio::test]
async fn service_status_answers_from_the_inventory() {
    use a2a_protocol_sdk::server::tool_loop::{Tool as _, ToolInvocation};
    let key = "0".repeat(64);
    let out = super::tools::ServiceStatus
        .call(ToolInvocation::new(
            serde_json::json!({"service": "checkout"}),
            &key,
        ))
        .await
        .unwrap();
    let out: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        out,
        serde_json::json!({
            "service": "checkout", "state": "healthy", "version": "2.8.0", "uptimeSeconds": 259_200
        })
    );
    assert!(
        super::tools::ServiceStatus
            .call(ToolInvocation::new(serde_json::json!({}), &key))
            .await
            .is_err()
    );
}
