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
    /// nothing is recorded.
    #[cfg(feature = "tracing")]
    pub fn recorder(&self) -> Option<OutputRecorder> {
        (!self.untraced).then(|| OutputRecorder::new(self.span.clone(), self.settings))
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
    /// The agent's output so far as message parts, the bytes left, and
    /// whether the turn has ended and been recorded.
    output: Mutex<(Vec<Value>, usize, bool)>,
}

#[cfg(feature = "tracing")]
impl OutputRecorder {
    pub const fn new(span: tracing::Span, settings: SpanSettings) -> Self {
        Self {
            span,
            content: settings.content,
            output: Mutex::new((Vec::new(), CONTENT_LIMIT, false)),
        }
    }

    /// Called with each event before it is written.
    ///
    /// The state and the output are recorded once, when the turn ends — a
    /// terminal state, a state that hands the turn back to the client, or a
    /// direct reply — because a field recorded twice is exported twice
    /// (`a2a.rs` says why), and because a long run then serialises its
    /// output once rather than per event. The first ending wins, as the
    /// task store's transition rules have it.
    pub fn observe(&self, event: &StreamResponse) {
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
        let ends_turn = reply || state.is_some_and(|s| s.is_terminal() || s.is_interrupted());
        let Ok(mut output) = self.output.lock() else {
            return;
        };
        let (collected, budget, ended) = &mut *output;
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
        if self.content && !collected.is_empty() {
            let mut message = json!({ "role": "assistant", "parts": collected });
            if let Some(reason) = state.and_then(finish_reason) {
                message["finish_reason"] = json!(reason);
            }
            self.span.record(
                "gen_ai.output.messages",
                json!([message]).to_string().as_str(),
            );
        }
    }
}

/// The conventions' `finish_reason` for a terminal state that has one.
/// `canceled`, `rejected` and the interrupted states have no counterpart in
/// its list (`stop`, `length`, `content_filter`, `tool_call`, `compaction`,
/// `error`), so
/// none is claimed for them.
#[cfg(feature = "tracing")]
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
mod tests {
    use super::*;

    #[test]
    fn every_part_kind_maps_to_a_conventions_part() {
        let parts = vec![
            Part::text("hi"),
            Part::url("https://x/y.png").with_media_type("image/png"),
            Part::raw("AAAA").with_media_type("application/pdf"),
            Part::data(json!({"k": 1})),
        ];
        let mut budget = CONTENT_LIMIT;
        let got = parts_json(&parts, &mut budget);
        assert_eq!(got[0], json!({"type": "text", "content": "hi"}));
        assert_eq!(
            got[1],
            json!({"type": "uri", "uri": "https://x/y.png", "mime_type": "image/png", "modality": "image"})
        );
        assert_eq!(got[2]["type"], "a2a.raw");
        assert_eq!(got[2]["base64_length"], 4);
        assert!(
            got[2].get("content").is_none(),
            "raw bytes must never be recorded"
        );
        assert_eq!(got[3], json!({"type": "a2a.data", "content": {"k": 1}}));
    }

    #[test]
    fn text_past_the_budget_is_cut_on_a_char_boundary_and_marked() {
        let mut budget = 5;
        // "héllo wörld": 'é' is two bytes, so byte 5 falls inside nothing
        // awkward but byte 2 would; cut at 5 lands after "héll".
        let cut = take("héllo wörld", &mut budget);
        assert!(cut.starts_with("héll"), "{cut}");
        assert!(cut.contains("truncated"), "{cut}");
        assert_eq!(budget, 0);
        let mut budget = 2;
        let cut = take("é!", &mut budget);
        assert!(cut.starts_with('é'), "{cut}");
        assert_eq!(take("rest", &mut budget), "…[truncated 4 bytes]");
    }

    /// What fits is charged to the budget exactly, for text and for data.
    #[test]
    fn what_fits_is_charged_exactly() {
        let mut budget = 10;
        assert_eq!(take("abc", &mut budget), "abc");
        assert_eq!(budget, 7);
        let mut budget = 100;
        // `{"a":1}` is seven bytes.
        let got = parts_json(&[Part::data(json!({"a": 1}))], &mut budget);
        assert_eq!(got[0]["content"], json!({"a": 1}));
        assert_eq!(budget, 93);
    }

    /// A budget ending inside a character cuts before it, not after.
    #[test]
    fn a_cut_inside_a_character_moves_back_to_its_start() {
        let mut budget = 2;
        // 'é' is bytes 1 and 2 of "aé"; byte 2 is inside it.
        assert_eq!(take("aé", &mut budget), "a…[truncated 2 bytes]");
    }

    #[test]
    fn data_that_does_not_fit_is_replaced_not_cut() {
        let mut budget = 3;
        let got = parts_json(&[Part::data(json!({"long": "value"}))], &mut budget);
        assert_eq!(got[0]["truncated"], true);
        assert_eq!(budget, 3);
    }

    /// The conventions name the agent's side `assistant`.
    #[test]
    fn roles_map_to_the_conventions_names() {
        assert_eq!(role(MessageRole::Agent), "assistant");
        assert_eq!(role(MessageRole::User), "user");
    }

    #[test]
    fn modality_follows_the_media_type() {
        assert_eq!(modality(Some("audio/ogg")), "audio");
        assert_eq!(modality(Some("video/mp4")), "video");
        assert_eq!(modality(Some("text/plain")), "document");
        assert_eq!(modality(None), "document");
    }

    #[test]
    fn only_completed_and_failed_claim_a_finish_reason() {
        assert_eq!(finish_reason(TaskState::Completed), Some("stop"));
        assert_eq!(finish_reason(TaskState::Failed), Some("error"));
        assert_eq!(finish_reason(TaskState::Canceled), None);
        assert_eq!(finish_reason(TaskState::InputRequired), None);
    }
}
