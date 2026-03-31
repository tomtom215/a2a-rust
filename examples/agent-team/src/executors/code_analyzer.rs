// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Code Analyzer executor — analyzes code metrics and streams artifact updates.
//!
//! **Dogfood note**: Demonstrates `boxed_future` helper and `EventEmitter` to
//! reduce boilerplate compared to raw `Box::pin(async move { ... })`.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::task::TaskState;

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::executor_helpers::boxed_future;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

use crate::helpers::EventEmitter;

/// Analyzes "code" sent as text parts. Counts lines, words, characters, and
/// streams incremental artifact updates (exercising append mode).
pub struct CodeAnalyzerExecutor;

impl AgentExecutor for CodeAnalyzerExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        boxed_future(async move {
            let emit = EventEmitter::new(ctx, queue);

            emit.status(TaskState::Working).await?;

            // Extract all text parts.
            let code: String = ctx
                .message
                .parts
                .iter()
                .filter_map(|p| match &p.content {
                    PartContent::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            let lines = code.lines().count();
            let words = code.split_whitespace().count();
            let chars = code.chars().count();

            // Check for cancellation between steps.
            if emit.is_cancelled() {
                emit.status(TaskState::Canceled).await?;
                return Ok(());
            }

            // Stream artifact 1: line count (append=false, first chunk).
            emit.artifact(
                "analysis-metrics",
                vec![Part::text(format!("Lines: {lines}"))],
                Some(false),
                Some(false),
            )
            .await?;

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;

            // Re-check cancellation between artifact emissions.
            if emit.is_cancelled() {
                emit.status(TaskState::Canceled).await?;
                return Ok(());
            }

            // Stream artifact 2: word/char counts (append=true, last chunk).
            emit.artifact(
                "analysis-metrics",
                vec![Part::text(format!("\nWords: {words}\nChars: {chars}"))],
                Some(true),
                Some(true),
            )
            .await?;

            // Also emit a JSON data artifact (exercises PartContent::Data).
            emit.artifact(
                "analysis-json",
                vec![Part::data(serde_json::json!({
                    "lines": lines,
                    "words": words,
                    "chars": chars,
                    "complexity": if lines > 50 { "high" } else { "low" },
                }))],
                None,
                Some(true),
            )
            .await?;

            emit.status(TaskState::Completed).await?;

            Ok(())
        })
    }
}
