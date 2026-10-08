// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The semantic conventions each call's spans must follow: the GenAI
//! `invoke_agent` span for the executor, and the draft A2A attributes on the
//! call's own span (ADR 0013, 2026-10-06).

use opentelemetry::trace::SpanKind;
use opentelemetry_sdk::trace::SpanData;

use crate::SEND_SPAN;
use crate::fixtures::attr;

/// Checks binding `b`'s executor span `span` and its call's `server` span.
pub fn check_call(b: &str, span: &SpanData, server: &SpanData, gaps: &mut Vec<String>) {
    // The GenAI conventions' internal `invoke_agent` span, which
    // is what an agent-aware backend keys on (Langfuse: AGENT,
    // and the context as its session).
    let context = attr(span, "a2a.context.id");
    for (key, want) in [
        ("gen_ai.operation.name", Some("invoke_agent".to_owned())),
        ("gen_ai.agent.name", Some("observed".to_owned())),
        ("gen_ai.agent.version", Some("1.0.0".to_owned())),
        ("gen_ai.conversation.id", context.clone()),
        ("a2a.task.state", Some("TASK_STATE_COMPLETED".to_owned())),
    ] {
        if attr(span, key) != want {
            gaps.push(format!(
                "{b}: the executor's span has {key} {:?}, not {want:?}",
                attr(span, key)
            ));
        }
    }
    if span.span_kind != SpanKind::Internal {
        gaps.push(format!(
            "{b}: the executor's span is {:?}, not INTERNAL",
            span.span_kind
        ));
    }
    // Content is opt-in: this handler did not opt in.
    for key in ["gen_ai.input.messages", "gen_ai.output.messages"] {
        if attr(span, key).is_some() {
            gaps.push(format!("{b}: {key} recorded without content capture"));
        }
    }
    // The call's own span carries the draft A2A conventions'
    // attributes (semantic-conventions-genai #195), the task and
    // context it ran.
    for (key, want) in [
        ("a2a.method.name", Some("SendMessage".to_owned())),
        ("a2a.protocol.version", Some("1.0".to_owned())),
        ("a2a.task.id", attr(span, "a2a.task.id")),
        ("gen_ai.conversation.id", context),
        ("gen_ai.agent.name", Some("observed".to_owned())),
        ("a2a.task.state", Some("TASK_STATE_COMPLETED".to_owned())),
    ] {
        if attr(server, key) != want {
            gaps.push(format!(
                "{b}: the server span has {key} {:?}, not {want:?}",
                attr(server, key)
            ));
        }
    }
    if attr(server, "a2a.message.id").is_none_or(|v| v.is_empty()) {
        gaps.push(format!("{b}: the server span has no a2a.message.id"));
    }
}

/// The call's `CLIENT` span sits under the caller's context, and the server
/// span under the client span, in the trace `trace_id`.
pub fn check_parentage(
    b: &str,
    trace_id: &str,
    parent_id: &str,
    server: &SpanData,
    spans: &[SpanData],
    gaps: &mut Vec<String>,
) {
    // The client records the call as a CLIENT span under the caller's
    // context, and sends that span as the `traceparent`: so the server
    // span's remote parent is the client's span, whose parent is the
    // caller's (O1, O8, O9).
    let client = spans.iter().find(|s| {
        s.span_kind == SpanKind::Client
            && s.name == SEND_SPAN
            && s.span_context.trace_id().to_string() == trace_id
    });
    match client {
        None => gaps.push(format!(
            "{b}: no CLIENT span `{SEND_SPAN}` in trace {} (O8)",
            trace_id
        )),
        Some(client) => {
            if client.parent_span_id.to_string() != parent_id {
                gaps.push(format!(
                    "{b}: the client span's parent is {}, not the caller's {} (O8)",
                    client.parent_span_id, parent_id
                ));
            }
            if server.parent_span_id != client.span_context.span_id()
                || !server.parent_span_is_remote
            {
                gaps.push(format!(
                    "{b}: the server span's parent is {} (remote: {}), not the client span {} (O1, O9)",
                    server.parent_span_id,
                    server.parent_span_is_remote,
                    client.span_context.span_id()
                ));
            }
        }
    }
}

/// No span carries an attribute key twice.
pub fn check_unique_keys(spans: &[SpanData], gaps: &mut Vec<String>) {
    // No span carries a key twice. `tracing-opentelemetry` appends a field
    // recorded twice as a second attribute with the same key, and a reader
    // taking the first then sees a stale value — which is how the executor's
    // `a2a.task.state` first read `TASK_STATE_WORKING` on a completed task.
    for span in spans {
        let mut keys: Vec<&str> = span.attributes.iter().map(|kv| kv.key.as_str()).collect();
        keys.sort_unstable();
        if let Some(pair) = keys.windows(2).find(|w| w[0] == w[1]) {
            gaps.push(format!("`{}` carries `{}` twice", span.name, pair[0]));
        }
    }
}
