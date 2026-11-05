// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The executor's span as the OpenTelemetry `GenAI` conventions' `invoke_agent`
//! span, and the messages it may carry.
//!
//! The executor *is* the agent: its span covers one run of it, in this
//! process. That is the conventions' "Invoke agent (internal)" span —
//! `docs/gen-ai/gen-ai-agent-spans.md` in
//! `open-telemetry/semantic-conventions-genai` at `4f85037` (2026-10-06):
//! kind `INTERNAL`, named `invoke_agent {gen_ai.agent.name}`, with
//! `gen_ai.operation.name` required and the agent's name, description and
//! conversation conditionally required. All of it is `development`.
//!
//! This is distinct from the A2A conventions' rule that A2A instrumentation
//! "SHOULD NOT report telemetry describing higher level `GenAI` agent
//! operations" (`model/a2a/spans.yaml`, pull request #195): that rule is
//! about the `SERVER` and `CLIENT` spans of an A2A request, which carry A2A
//! attributes only (`a2a.rs`). An agent framework running inside the
//! executor may report `invoke_agent` itself, in which case two would nest;
//! `RequestHandlerBuilder::with_agent_span_conventions(false)` turns this
//! one off.
//!
//! Observability backends key on `gen_ai.operation.name`: Langfuse maps
//! `invoke_agent` to an `AGENT` observation and `gen_ai.conversation.id` to
//! its session (`ObservationTypeMapper.ts` and `extractSessionId` in
//! `langfuse/langfuse@1a21a42`).
//!
//! **Content is opt-in.** `gen_ai.input.messages` and
//! `gen_ai.output.messages` are Opt-In in the conventions because messages
//! carry whatever users and agents say; with
//! `RequestHandlerBuilder::with_span_content_capture(true)` the request's
//! message and the agent's replies and artifacts are recorded, in the
//! conventions' JSON message format (`model/gen-ai/gen-ai-*-messages.json`),
//! within [`CONTENT_LIMIT`] bytes per attribute. Raw bytes are never
//! recorded, only their type and size.

use std::future::Future;
#[cfg(feature = "tracing")]
use std::sync::Mutex;

use a2a_protocol_types::agent_card::AgentCard;
#[cfg(feature = "tracing")]
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::Message;
#[cfg(feature = "tracing")]
use a2a_protocol_types::message::{MessageRole, Part, PartContent};
#[cfg(feature = "tracing")]
use a2a_protocol_types::task::TaskState;
#[cfg(feature = "tracing")]
use serde_json::{Value, json};

#[cfg(feature = "tracing")]
mod lineage;

/// The most bytes of text and structured data one message attribute holds.
/// Langfuse's own default field limit is 2 MiB
/// (`LANGFUSE_OBSERVATION_FIELD_SIZE_LIMIT_BYTES`); a span attribute is held
/// in memory until export, so this is far below it.
#[cfg(feature = "tracing")]
pub const CONTENT_LIMIT: usize = 64 * 1024;

/// What the executor span records; set on `RequestHandlerBuilder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpanSettings {
    /// Record the executor span as `invoke_agent`. Default on.
    pub agent: bool,
    /// Record input and output messages. Default off.
    pub content: bool,
}

impl Default for SpanSettings {
    fn default() -> Self {
        Self {
            agent: true,
            content: false,
        }
    }
}

/// The executor's span for one run, opened before the executor is spawned
/// and entered by the future that runs it.
#[derive(Debug)]
pub struct ExecutorSpan {
    #[cfg(feature = "tracing")]
    span: tracing::Span,
    #[cfg(feature = "tracing")]
    untraced: bool,
    #[cfg(feature = "tracing")]
    settings: SpanSettings,
}

impl ExecutorSpan {
    /// Opens it — or, for a call under `InboundTracePolicy::Drop`, a span
    /// that records nothing, as that policy promises.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::missing_const_for_fn)
    )]
    pub fn open(
        task_id: &str,
        context_id: &str,
        card: Option<&AgentCard>,
        settings: SpanSettings,
        input: &Message,
    ) -> Self {
        #[cfg(feature = "tracing")]
        {
            let untraced = super::untraced();
            let span = if untraced {
                tracing::Span::none()
            } else {
                executor_span(task_id, context_id, card, settings, input)
            };
            Self {
                span,
                untraced,
                settings,
            }
        }
        #[cfg(not(feature = "tracing"))]
        {
            Self {}
        }
    }

    /// What watches the executor's writes on this span's behalf; `None` when
    /// nothing is recorded. A run that `continues` a task has no direct reply.
    #[cfg(feature = "tracing")]
    pub fn recorder(&self, continues: bool) -> Option<OutputRecorder> {
        (!self.untraced).then(|| OutputRecorder::new(self.span.clone(), self.settings, continues))
    }

    /// Runs `fut` — the executor's whole run — inside the span, carrying the
    /// call's trace policy across the spawn.
    #[cfg_attr(not(feature = "tracing"), allow(clippy::unused_self))]
    pub fn instrument<F: Future>(self, fut: F) -> impl Future<Output = F::Output> {
        #[cfg(feature = "tracing")]
        {
            tracing::Instrument::instrument(super::UNTRACED.scope(self.untraced, fut), self.span)
        }
        #[cfg(not(feature = "tracing"))]
        {
            fut
        }
    }
}

/// Opens the executor's span for one run.
#[cfg(feature = "tracing")]
fn executor_span(
    task_id: &str,
    context_id: &str,
    card: Option<&AgentCard>,
    settings: SpanSettings,
    input: &Message,
) -> tracing::Span {
    let span = tracing::info_span!(
        target: "a2a_protocol_server::rpc",
        "a2a.task",
        otel.name = tracing::field::Empty,
        a2a.task.id = task_id,
        a2a.context.id = context_id,
        a2a.task.state = tracing::field::Empty,
        a2a.lineage.root = tracing::field::Empty,
        a2a.lineage.depth = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
        error.type = tracing::field::Empty,
        gen_ai.operation.name = tracing::field::Empty,
        gen_ai.conversation.id = tracing::field::Empty,
        gen_ai.agent.name = tracing::field::Empty,
        gen_ai.agent.description = tracing::field::Empty,
        gen_ai.agent.version = tracing::field::Empty,
        gen_ai.input.messages = tracing::field::Empty,
        gen_ai.output.messages = tracing::field::Empty,
    );
    // With nothing recording, build nothing: the name is formatted and the
    // input serialised only for a span someone will see. Recorded before the
    // span is first entered, while `tracing-opentelemetry` still holds it as
    // a builder, which is when it takes `otel.name`.
    if span.is_disabled() {
        return span;
    }
    match (settings.agent, card) {
        (false, _) => span.record("otel.name", "a2a.execute"),
        (true, Some(card)) => {
            span.record("otel.name", format!("invoke_agent {}", card.name).as_str())
        }
        (true, None) => span.record("otel.name", "invoke_agent"),
    };
    if settings.agent {
        span.record("gen_ai.operation.name", "invoke_agent");
        span.record("gen_ai.conversation.id", context_id);
        super::a2a::record_agent(&span, card);
    }
    if settings.content {
        let mut budget = CONTENT_LIMIT;
        let input = json!([{
            "role": role(input.role),
            "parts": parts_json(&input.parts, &mut budget),
        }]);
        span.record("gen_ai.input.messages", input.to_string().as_str());
    }
    span
}

/// Watches what the executor writes, and records the outcome on its span.
#[cfg(feature = "tracing")]
#[derive(Debug)]
pub struct OutputRecorder {
    span: tracing::Span,
    content: bool,
    /// Output parts, bytes left, turn ended, and task stream (a continued
    /// task, or task/status/artifact seen: a `Message` is no reply, §3.1.2).
    output: Mutex<(Vec<Value>, usize, bool, bool)>,
}

#[cfg(feature = "tracing")]
impl OutputRecorder {
    pub const fn new(span: tracing::Span, settings: SpanSettings, continues: bool) -> Self {
        Self {
            span,
            content: settings.content,
            output: Mutex::new((Vec::new(), CONTENT_LIMIT, false, continues)),
        }
    }

    /// Called with each event before it is written.
    ///
    /// The state and the output are recorded once, when the turn ends — a
    /// terminal state, a state that hands the turn back to the client, or a
    /// direct reply — because a field recorded twice is exported twice
    /// (`a2a.rs` says why), and because a long run then serialises its
    /// output once rather than per event. The first ending wins, as the
    /// task store's transition rules have it. A turn ending `Failed` — the
    /// executor's own status, or the one written for an executor that
    /// returned `Err` or timed out — marks the span failed: `Error` status and
    /// an `error.type` ([`failure_type`]), which the `GenAI` conventions
    /// require "if the operation ended in an error".
    pub fn observe(&self, event: &StreamResponse) {
        let status = match event {
            StreamResponse::Task(t) => Some(&t.status),
            StreamResponse::StatusUpdate(u) => Some(&u.status),
            _ => None,
        };
        let (state, parts, reply) = match event {
            StreamResponse::Task(t) => (Some(t.status.state), None, false),
            StreamResponse::StatusUpdate(u) => (
                Some(u.status.state),
                u.status
                    .message
                    .as_ref()
                    .filter(|m| m.role == MessageRole::Agent)
                    .map(|m| &m.parts),
                false,
            ),
            StreamResponse::ArtifactUpdate(a) => (None, Some(&a.artifact.parts), false),
            StreamResponse::Message(m) => (None, Some(&m.parts), true),
            _ => (None, None, false),
        };
        let Ok(mut output) = self.output.lock() else {
            return;
        };
        let (collected, budget, ended, task_stream) = &mut *output;
        let reply = reply && !*task_stream;
        *task_stream |= !matches!(event, StreamResponse::Message(_));
        let ends_turn = reply || state.is_some_and(|s| s.is_terminal() || s.is_interrupted());
        if *ended {
            return;
        }
        if self.content
            && let Some(parts) = parts
        {
            collected.extend(parts_json(parts, budget));
        }
        if !ends_turn {
            return;
        }
        *ended = true;
        if let Some(state) = state {
            self.span
                .record("a2a.task.state", super::a2a::task_state(state));
        }
        if let Some(status) = status.filter(|s| s.state == TaskState::Failed) {
            self.span.record("error.type", failure_type(status));
            self.span.record("otel.status_code", "ERROR");
        }
        #[cfg(feature = "otel")]
        if let Some(reason) = state.and_then(finish_reason) {
            super::a2a::record_finish_reason(&self.span, reason);
        }
        if self.content && !collected.is_empty() {
            let message = json!({ "role": "assistant", "parts": collected });
            self.span.record(
                "gen_ai.output.messages",
                json!([message]).to_string().as_str(),
            );
        }
    }
}

/// The `error.type` of a turn that ended `Failed`: the failure class the
/// status message carries (`a2a_protocol_types::failure`) — `internal`,
/// `invalid-request`, `transient`, `policy-refusal` or `budget-exhausted`,
/// the class an executor timeout is given — and `internal` for a status that
/// classifies nothing, as that module reads one. A bounded list, as the
/// `GenAI` conventions ask ("another low-cardinality error identifier").
#[cfg(feature = "tracing")]
fn failure_type(status: &a2a_protocol_types::task::TaskStatus) -> &'static str {
    use a2a_protocol_types::failure::{FailureClass, class_of};
    status
        .message
        .as_ref()
        .and_then(class_of)
        .unwrap_or(FailureClass::Internal)
        .as_str()
}

/// The conventions' finish reason for a terminal state that has one, as
/// `gen_ai.response.finish_reasons` records it.
/// `canceled`, `rejected` and the interrupted states have no counterpart in
/// its list (`stop`, `length`, `content_filter`, `tool_call`, `compaction`,
/// `error`), so
/// none is claimed for them.
#[cfg(feature = "otel")]
const fn finish_reason(state: TaskState) -> Option<&'static str> {
    match state {
        TaskState::Completed => Some("stop"),
        TaskState::Failed => Some("error"),
        _ => None,
    }
}

#[cfg(feature = "tracing")]
const fn role(role: MessageRole) -> &'static str {
    match role {
        MessageRole::Agent => "assistant",
        // `User`, and the proto default, which a valid request never sends.
        _ => "user",
    }
}

#[cfg(feature = "tracing")]
/// The conventions' `modality` for a media type: image, video and audio by
/// their top-level type, anything else a document.
fn modality(media_type: Option<&str>) -> &'static str {
    match media_type.and_then(|m| m.split('/').next()) {
        Some("image") => "image",
        Some("video") => "video",
        Some("audio") => "audio",
        _ => "document",
    }
}

#[cfg(feature = "tracing")]
/// `parts` as the conventions' message parts, spending at most `budget`
/// bytes on text and data; whatever does not fit is cut, and says so.
fn parts_json(parts: &[Part], budget: &mut usize) -> Vec<Value> {
    parts
        .iter()
        .map(|part| {
            let media = part.media_type.as_deref();
            match &part.content {
                PartContent::Text(text) => json!({ "type": "text", "content": take(text, budget) }),
                PartContent::Url(url) => json!({
                    "type": "uri",
                    "uri": take(url, budget),
                    "mime_type": media,
                    "modality": modality(media),
                }),
                // Bytes are not content anyone reads in a trace, and an
                // inline file would dwarf every other attribute.
                PartContent::Raw(base64) => json!({
                    "type": "a2a.raw",
                    "mime_type": media,
                    "filename": part.filename,
                    "base64_length": base64.len(),
                }),
                PartContent::Data(value) => {
                    let text = value.to_string();
                    if text.len() <= *budget {
                        *budget -= text.len();
                        json!({ "type": "a2a.data", "content": value })
                    } else {
                        json!({ "type": "a2a.data", "truncated": true, "length": text.len() })
                    }
                }
                _ => json!({ "type": "a2a.unknown" }),
            }
        })
        .collect()
}

#[cfg(feature = "tracing")]
/// `s`, or as much of it as `budget` allows on a character boundary followed
/// by a marker, and charges the budget.
fn take(s: &str, budget: &mut usize) -> String {
    if s.len() <= *budget {
        *budget -= s.len();
        return s.to_owned();
    }
    let mut end = *budget;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    *budget = 0;
    format!("{}…[truncated {} bytes]", &s[..end], s.len() - end)
}

#[cfg(all(test, feature = "tracing"))]
mod tests;
