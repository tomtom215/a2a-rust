// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An executor that fails, and one that times out: the `GenAI` conventions'
//! `invoke_agent` span requires `error.type` "if the operation ended in an
//! error", and the span's status is then `Error` (`recording-errors.md`).
//! The task still answers `Failed` either way; what this checks is that the
//! span says so too.

use std::time::Duration;

use a2a_protocol_sdk::server::{RequestHandlerBuilder, agent_executor};
use a2a_protocol_sdk::types::{A2aError, AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};

use crate::fixtures::{attr, message};

/// The executor span of the agent below.
const SPAN: &str = "invoke_agent failing";

/// Returns `Err` for `fail`; never returns for `hang`.
struct Failing;

agent_executor!(Failing, |ctx, _queue| async {
    if ctx.message.text() == Some("hang") {
        std::future::pending::<()>().await;
    }
    Err(A2aError::internal("the agent broke"))
});

/// Runs both through a handler of their own, as a blocking `SendMessage`.
pub async fn run() {
    let card = AgentCard::new(
        "failing",
        "1.0.0",
        AgentInterface::jsonrpc("http://127.0.0.1"),
    )
    .with_description("An agent that fails")
    .with_skill(AgentSkill::new("fail", "Fail", "Fails").with_tags(["fail"]))
    .with_capabilities(AgentCapabilities::none());
    let handler = RequestHandlerBuilder::new(Failing)
        .with_agent_card(card)
        .with_executor_timeout(Duration::from_millis(200))
        .build()
        .expect("handler");
    for text in ["fail", "hang"] {
        // The task answers `Failed`; the call itself succeeds.
        let _ = handler
            .on_send_message(message(text, None), false, None)
            .await
            .unwrap_or_else(|e| panic!("{text}: {e}"));
    }
}

/// Both runs' spans: `Error` status, and the failure class as `error.type`
/// — `internal` for the `Err`, `budget-exhausted` for the deadline.
pub async fn check(
    exporter: &InMemorySpanExporter,
    provider: &SdkTracerProvider,
    gaps: &mut Vec<String>,
) {
    // Exported when the executor's run ends, which may be just after the
    // call answered: wait for both, with a deadline.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let spans = loop {
        let _ = provider.force_flush();
        let spans: Vec<_> = exporter
            .get_finished_spans()
            .expect("spans")
            .into_iter()
            .filter(|s| s.name == SPAN)
            .collect();
        if spans.len() >= 2 || tokio::time::Instant::now() >= deadline {
            break spans;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    };
    if spans.len() != 2 {
        gaps.push(format!(
            "executor errors: {} `{SPAN}` spans, not 2",
            spans.len()
        ));
    }
    let mut types: Vec<_> = spans.iter().filter_map(|s| attr(s, "error.type")).collect();
    types.sort();
    if types != ["budget-exhausted", "internal"] {
        gaps.push(format!(
            "executor errors: the failed and timed-out runs' error.type are {types:?}, \
             not [\"budget-exhausted\", \"internal\"]"
        ));
    }
    for span in &spans {
        if !matches!(span.status, opentelemetry::trace::Status::Error { .. }) {
            gaps.push(format!(
                "executor errors: a failed run's `{SPAN}` span has status {:?}, not Error",
                span.status
            ));
        }
    }
}
