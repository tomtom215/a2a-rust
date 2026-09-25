// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The ACTS system-under-test behaviours (ACTS §11.1).
//!
//! The ACTS conformance corpus (a2aproject/a2a-itk, `scenarios/acts/`) drives
//! the agent with plain-text messages that begin with a `tck-*` prefix and
//! expects each to produce a scripted outcome: complete, ask for input, fail,
//! stream an artifact in chunks, and so on. `../acts/sut-behaviors.yaml`
//! declares which prefixes this agent implements; the runner fails any test
//! needing one it does not declare. This module is the implementation, and
//! the two lists are kept identical.
//!
//! A follow-up turn need not repeat the prefix (`"here is more input"`), so
//! the behaviour of a continuation is the one the task's first message chose,
//! read back from the stored task's history.

use std::time::Duration;

use base64::Engine as _;

use a2a_protocol_server::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::task::{ContextId, TaskState};

use crate::ItkExecutor;

/// Every behaviour this agent implements, by prefix. Longest first, so that
/// no prefix shadows a longer one that begins with it.
const BEHAVIOURS: [&str; 15] = [
    "tck-artifact-file-url",
    "tck-message-response",
    "tck-artifact-text",
    "tck-artifact-data",
    "tck-artifact-file",
    "tck-complete-task",
    "tck-input-required",
    "tck-stream-chunked",
    "tck-auth-required",
    "tck-long-running",
    "tck-task-failure",
    "tck-reject-task",
    "tck-stream-basic",
    "tck-multi-turn",
    "tck-cancel",
];

/// How long `tck-long-running` stays in WORKING (the contract's `delay_ms`).
const LONG_RUNNING: Duration = Duration::from_millis(1000);

/// How long `tck-cancel` waits for its cancel before giving up, so a run
/// whose cancel never arrives does not leave the task running for ever.
const CANCEL_WAIT: Duration = Duration::from_secs(120);

fn texts(message: &Message) -> impl Iterator<Item = &str> {
    message.parts.iter().filter_map(Part::text_content)
}

fn prefix_of(text: &str) -> Option<&'static str> {
    let text = text.trim_start();
    BEHAVIOURS.iter().copied().find(|p| text.starts_with(p))
}

/// The behaviour this turn runs, and whether it is a continuation: from the
/// message's own text, else from the first user message in the stored task's
/// history that names one.
pub(crate) fn behaviour(ctx: &RequestContext) -> Option<(&'static str, bool)> {
    if let Some(p) = texts(&ctx.message).find_map(prefix_of) {
        return Some((p, ctx.stored_task.is_some()));
    }
    let history = ctx.stored_task.as_ref()?.history.as_ref()?;
    history
        .iter()
        .filter(|m| m.role == MessageRole::User)
        .flat_map(texts)
        .find_map(prefix_of)
        .map(|p| (p, true))
}

fn agent_message(ctx: &RequestContext, text: &str) -> Message {
    Message {
        id: MessageId::new(format!("acts-{}", crate::uuid_v4())),
        role: MessageRole::Agent,
        parts: vec![Part::text(text)],
        task_id: Some(ctx.task_id.clone()),
        context_id: Some(ContextId::new(ctx.context_id.clone())),
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

async fn status(
    ctx: &RequestContext,
    queue: &dyn EventQueueWriter,
    state: TaskState,
    text: &str,
) -> A2aResult<()> {
    queue
        .write(ItkExecutor::status_event(ctx, state, Some(text.to_owned())))
        .await
}

async fn artifact(
    ctx: &RequestContext,
    queue: &dyn EventQueueWriter,
    id: &str,
    part: Part,
    append: bool,
    last_chunk: bool,
) -> A2aResult<()> {
    queue
        .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            artifact: Artifact::new(id, vec![part]),
            append: Some(append),
            last_chunk: Some(last_chunk),
            metadata: None,
        }))
        .await
}

/// Runs `behaviour` for this turn.
pub(crate) async fn run(
    ctx: &RequestContext,
    queue: &dyn EventQueueWriter,
    behaviour: &'static str,
    continuation: bool,
) -> A2aResult<()> {
    use TaskState::{
        AuthRequired, Canceled, Completed, Failed, InputRequired, Rejected, Working,
    };

    if behaviour == "tck-message-response" {
        return queue
            .write(StreamResponse::Message(agent_message(ctx, "tck-message-response ok")))
            .await;
    }

    status(ctx, queue, Working, "working").await?;
    match behaviour {
        "tck-complete-task" => status(ctx, queue, Completed, "task completed").await,
        "tck-input-required" if continuation => {
            status(ctx, queue, Completed, "input received").await
        }
        "tck-input-required" => status(ctx, queue, InputRequired, "more input needed").await,
        "tck-auth-required" => status(ctx, queue, AuthRequired, "authentication needed").await,
        "tck-reject-task" => status(ctx, queue, Rejected, "task rejected").await,
        "tck-task-failure" => status(ctx, queue, Failed, "task failed on request").await,
        "tck-multi-turn" => {
            let done = texts(&ctx.message)
                .any(|t| t.split_whitespace().any(|w| w.eq_ignore_ascii_case("done")));
            if done {
                status(ctx, queue, Completed, "conversation complete").await
            } else {
                status(ctx, queue, InputRequired, "send 'done' to finish").await
            }
        }
        "tck-cancel" => {
            tokio::select! {
                () = ctx.cancellation_token.cancelled() => {
                    status(ctx, queue, Canceled, "task canceled").await
                }
                () = tokio::time::sleep(CANCEL_WAIT) => {
                    status(ctx, queue, Failed, "no cancel arrived").await
                }
            }
        }
        "tck-long-running" => {
            tokio::select! {
                () = ctx.cancellation_token.cancelled() => {
                    status(ctx, queue, Canceled, "task canceled").await
                }
                () = tokio::time::sleep(LONG_RUNNING) => {
                    status(ctx, queue, Completed, "long-running task completed").await
                }
            }
        }
        "tck-artifact-text" => {
            artifact(ctx, queue, "text", Part::text("generated text content"), false, true)
                .await?;
            status(ctx, queue, Completed, "artifact produced").await
        }
        "tck-artifact-data" => {
            let data = serde_json::json!({"key": "value", "count": 1});
            artifact(ctx, queue, "data", Part::data(data), false, true).await?;
            status(ctx, queue, Completed, "artifact produced").await
        }
        "tck-artifact-file" => {
            let bytes = base64::engine::general_purpose::STANDARD.encode("document content");
            let part = Part::raw(bytes)
                .with_filename("document.txt")
                .with_media_type("text/plain");
            artifact(ctx, queue, "file", part, false, true).await?;
            status(ctx, queue, Completed, "artifact produced").await
        }
        "tck-artifact-file-url" => {
            let part = Part::url("https://example.com/document.txt")
                .with_filename("document.txt")
                .with_media_type("text/plain");
            artifact(ctx, queue, "file-url", part, false, true).await?;
            status(ctx, queue, Completed, "artifact produced").await
        }
        "tck-stream-basic" => {
            artifact(ctx, queue, "stream", Part::text("streamed content"), false, true).await?;
            status(ctx, queue, Completed, "stream complete").await
        }
        "tck-stream-chunked" => {
            let chunks = ["chunk one ", "chunk two ", "chunk three"];
            for (i, chunk) in chunks.iter().enumerate() {
                artifact(ctx, queue, "chunked", Part::text(*chunk), i > 0, i + 1 == chunks.len())
                    .await?;
            }
            status(ctx, queue, Completed, "stream complete").await
        }
        other => {
            status(ctx, queue, Failed, &format!("behaviour {other} is declared but not handled"))
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BEHAVIOURS;

    /// `acts/sut-behaviors.yaml` is what the runner believes; `BEHAVIOURS`
    /// is what this agent does. A prefix in one and not the other either
    /// fails tests the agent would pass or claims support it lacks.
    #[test]
    fn the_contract_file_declares_exactly_the_implemented_behaviours() {
        let contract = include_str!("../../acts/sut-behaviors.yaml");
        let mut declared: Vec<&str> = contract
            .lines()
            .filter_map(|l| l.trim().strip_prefix("- prefix: "))
            .map(|p| p.trim_matches('"'))
            .collect();
        declared.sort_unstable();
        let mut implemented = BEHAVIOURS.to_vec();
        implemented.sort_unstable();
        assert_eq!(declared, implemented);
    }

    #[test]
    fn no_prefix_shadows_a_longer_one() {
        for (i, short) in BEHAVIOURS.iter().enumerate() {
            for long in &BEHAVIOURS[i + 1..] {
                assert!(
                    !long.starts_with(short),
                    "{short} precedes {long}, which it would shadow"
                );
            }
        }
    }
}
