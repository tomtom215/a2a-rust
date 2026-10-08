// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The A2A attributes on a call's `SERVER` span, as the draft OpenTelemetry
//! A2A conventions define them.
//!
//! The source is `open-telemetry/semantic-conventions-genai` pull request
//! #195 ("semconv for a2a protocol"), read at its head `842a839` on
//! 2026-10-06: `model/a2a/{registry,common,spans}.yaml`. It is open and every
//! attribute in it is `development`, so these names may still change; the
//! pull request is the place to follow them. Two of its rules are applied as
//! written: a transport's server span is *enriched* rather than a nested
//! `a2a.server` span reported, and the gRPC span keeps its gRPC name. Its
//! third, renaming an HTTP server span to `{a2a.method.name}`, is not: ADR
//! 0013 gives a call one span name on every binding, and that stands until
//! the conventions are settled.
//!
//! **Each attribute is recorded once per span.** `tracing-opentelemetry`
//! 0.33.0 appends a field recorded twice as a second attribute with the same
//! key (`layer.rs`, `SpanBuilderUpdates::update`), and a reader taking the
//! first sees the stale value — so every value here is recorded at the one
//! point it is final, never refined later.
//!
//! Recording goes to [`tracing::Span::current`], which while a handler method
//! runs is the call's span ([`ServerSpan::run`](super::ServerSpan::run)
//! instruments it). Every field is declared when that span opens; `record`
//! on a span that did not declare a field — a span of the application's own
//! that happens to be current — is a no-op, so a misplaced call records
//! nothing rather than something wrong.

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::task::TaskState;

/// The fully qualified gRPC method's prefix, which `a2a.method.name` drops.
const SERVICE_PREFIX: &str = "lf.a2a.v1.A2AService/";

/// `a2a.method.name` for a qualified method name: `SendMessage` for
/// `lf.a2a.v1.A2AService/SendMessage`; `None` for `_OTHER`.
pub fn method_name(qualified: &str) -> Option<&str> {
    qualified.strip_prefix(SERVICE_PREFIX)
}

/// `a2a.task.state`: the protobuf enum name, which is also how this crate
/// serialises the state, so the two cannot disagree.
pub const fn task_state(state: TaskState) -> &'static str {
    match state {
        TaskState::Submitted => "TASK_STATE_SUBMITTED",
        TaskState::Working => "TASK_STATE_WORKING",
        TaskState::InputRequired => "TASK_STATE_INPUT_REQUIRED",
        TaskState::AuthRequired => "TASK_STATE_AUTH_REQUIRED",
        TaskState::Completed => "TASK_STATE_COMPLETED",
        TaskState::Failed => "TASK_STATE_FAILED",
        TaskState::Canceled => "TASK_STATE_CANCELED",
        TaskState::Rejected => "TASK_STATE_REJECTED",
        // `TaskState` is `#[non_exhaustive]`; a state added later is
        // unspecified until it is mapped here.
        _ => "TASK_STATE_UNSPECIFIED",
    }
}

/// The agent's identity from its card, as `gen_ai.agent.*`: "when available
/// from the Agent Card". An empty description or version is not available.
pub fn record_agent(span: &tracing::Span, card: Option<&AgentCard>) {
    let Some(card) = card else { return };
    span.record("gen_ai.agent.name", card.name.as_str());
    if !card.description.is_empty() {
        span.record("gen_ai.agent.description", card.description.as_str());
    }
    if !card.version.is_empty() {
        span.record("gen_ai.agent.version", card.version.as_str());
    }
}

/// `a2a.tenant`, when the request's `tenant` field is set.
pub fn record_tenant(tenant: Option<&str>) {
    if let Some(t) = tenant.filter(|t| !t.is_empty()) {
        tracing::Span::current().record("a2a.tenant", t);
    }
}

/// `a2a.message.id` and `a2a.message.reference_task_ids`, for a request
/// carrying a message. Its context is recorded with the task, once the
/// server has resolved it.
pub fn record_message(message: &a2a_protocol_types::message::Message) {
    let span = tracing::Span::current();
    span.record("a2a.message.id", message.id.0.as_str());
    #[cfg(feature = "otel")]
    if let Some(ids) = message
        .reference_task_ids
        .as_ref()
        .filter(|ids| !ids.is_empty())
    {
        // A string array: a `tracing` field holds one value, so this goes
        // through the OpenTelemetry span directly.
        use tracing_opentelemetry::OpenTelemetrySpanExt as _;
        let values: Vec<opentelemetry::StringValue> =
            ids.iter().map(|id| id.0.clone().into()).collect();
        span.set_attribute(
            "a2a.message.reference_task_ids",
            opentelemetry::Value::Array(values.into()),
        );
    }
}

/// `a2a.task.id`, and `gen_ai.conversation.id` when the context is known.
pub fn record_task(task_id: &str, context_id: Option<&str>) {
    let span = tracing::Span::current();
    span.record("a2a.task.id", task_id);
    if let Some(context) = context_id {
        span.record("gen_ai.conversation.id", context);
    }
}

/// `a2a.task.state`, for a response carrying a task whose id was recorded
/// already.
pub fn record_task_state(state: TaskState) {
    tracing::Span::current().record("a2a.task.state", task_state(state));
}

/// [`record_task`] and [`record_task_state`], for a response carrying a task
/// nothing earlier in the call recorded.
pub fn record_task_response(task: &a2a_protocol_types::task::Task) {
    record_task(&task.id.0, Some(&task.context_id.0));
    record_task_state(task.status.state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_name_is_the_unqualified_method_and_none_for_other() {
        assert_eq!(
            method_name("lf.a2a.v1.A2AService/SendMessage"),
            Some("SendMessage")
        );
        assert_eq!(
            method_name("lf.a2a.v1.A2AService/ListTaskPushNotificationConfigs"),
            Some("ListTaskPushNotificationConfigs")
        );
        assert_eq!(method_name("_OTHER"), None);
    }

    /// The mapping must agree with the serialised form, which is the
    /// conventions' value set (`TASK_STATE_*`), for every state.
    #[test]
    fn task_state_matches_the_wire_spelling_for_every_state() {
        for state in [
            TaskState::Unspecified,
            TaskState::Submitted,
            TaskState::Working,
            TaskState::InputRequired,
            TaskState::AuthRequired,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Canceled,
            TaskState::Rejected,
        ] {
            let wire = serde_json::to_value(state).expect("serialise");
            assert_eq!(wire.as_str(), Some(task_state(state)), "{state:?}");
        }
    }
}
