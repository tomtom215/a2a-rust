// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `CLIENT` span each call runs in, and the trace context it sends.
//!
//! One span per A2A call, opened after the interceptors have run and closed
//! when the response is in — or, for a streaming method, when the stream is
//! dropped, which is when the caller has finished consuming it. Retries
//! inside the transport are one call. Named, like the server's span, for the
//! fully qualified gRPC method, so a call and the server span it caused share
//! a name on every binding (ADR 0013).
//!
//! The attributes are the draft OpenTelemetry A2A conventions' `a2a.client`
//! span (`open-telemetry/semantic-conventions-genai` pull request #195 at
//! `842a839`, `model/a2a/spans.yaml`): `a2a.method.name`,
//! `a2a.protocol.version`, the message, task and context, the agent from its
//! card when the client was built from one, `server.address` and
//! `server.port`, and `error.type` on failure. As that file requires of A2A
//! instrumentation, nothing here reports a `GenAI` agent operation.
//!
//! Each attribute is recorded once, at the end, when its final value is
//! known: `tracing-opentelemetry` 0.33.0 exports a field recorded twice as two
//! attributes with one key.
//!
//! With the `otel` feature and a `tracing-opentelemetry` layer recording,
//! the call's `traceparent` names this span — so the agent called is a child
//! of the call, not of whatever was current — on the transports this crate
//! builds for a binding with per-request headers (JSON-RPC, HTTP+JSON,
//! gRPC). [`ClientConfig::with_trace_propagation`](crate::ClientConfig::with_trace_propagation)
//! turns that off for a client calling agents that should not learn its
//! trace ids.

use std::sync::Arc;

use a2a_protocol_types::agent_card::AgentCard;

/// The agent's name, description and version from `card`, as a client's
/// spans carry them.
pub fn agent_identity(card: &AgentCard) -> Arc<(String, String, String)> {
    Arc::new((
        card.name.clone(),
        card.description.clone(),
        card.version.clone(),
    ))
}

/// What a client knows about the agent it calls, for its spans.
#[derive(Clone, Debug, Default)]
#[cfg_attr(not(feature = "tracing"), allow(dead_code))]
pub struct Peer {
    /// `rpc.system.name`, when the client chose the transport.
    pub(crate) system: Option<&'static str>,
    pub(crate) address: Option<String>,
    pub(crate) port: Option<u16>,
    /// Name, description and version, from the card.
    pub(crate) agent: Option<Arc<(String, String, String)>>,
    /// Whether the transport carries per-request headers, so a
    /// `traceparent` set for one call reaches the agent with that call.
    #[cfg_attr(not(feature = "otel"), allow(dead_code))]
    pub(crate) carries_headers: bool,
}

impl Peer {
    /// The peer of a client this crate built a transport for.
    pub(crate) fn new(
        binding: &str,
        endpoint: &str,
        agent: Option<Arc<(String, String, String)>>,
    ) -> Self {
        let system = match binding.to_ascii_uppercase().as_str() {
            "JSONRPC" => Some("jsonrpc"),
            "HTTP+JSON" | "REST" => Some("a2a_http_json"),
            "GRPC" => Some("grpc"),
            _ => None,
        };
        let uri = endpoint.parse::<hyper::Uri>().ok();
        let address = uri
            .as_ref()
            .and_then(|u| u.host())
            .map(|h| h.trim_matches(['[', ']']).to_owned());
        let port = uri.as_ref().and_then(|u| {
            u.port_u16().or_else(|| match u.scheme_str() {
                Some("https") => Some(443),
                Some("http") => Some(80),
                _ => None,
            })
        });
        Self {
            system,
            address,
            port,
            agent,
            carries_headers: system.is_some(),
        }
    }
}

/// A call's span, from open to the recording of its outcome.
#[cfg(feature = "tracing")]
#[derive(Debug)]
pub struct CallSpan {
    pub(crate) span: tracing::Span,
    task_id: Option<String>,
    context_id: Option<String>,
}

#[cfg(feature = "tracing")]
impl CallSpan {
    /// Opens the span for `method` and reads what the request says about
    /// its message, task and context.
    pub(crate) fn open(method: &str, peer: &Peer, params: &serde_json::Value) -> Self {
        let qualified = qualified(method);
        let span = tracing::info_span!(
            target: "a2a_protocol_client::rpc",
            "a2a.rpc.client",
            otel.name = qualified,
            otel.kind = "client",
            otel.status_code = tracing::field::Empty,
            rpc.system.name = peer.system,
            rpc.method = qualified,
            a2a.method.name = method,
            a2a.protocol.version = a2a_protocol_types::A2A_VERSION,
            server.address = peer.address.as_deref(),
            server.port = peer.port,
            error.type = tracing::field::Empty,
            a2a.message.id = tracing::field::Empty,
            a2a.task.id = tracing::field::Empty,
            a2a.task.state = tracing::field::Empty,
            gen_ai.conversation.id = tracing::field::Empty,
            gen_ai.agent.name = tracing::field::Empty,
            gen_ai.agent.description = tracing::field::Empty,
            gen_ai.agent.version = tracing::field::Empty,
        );
        // With nothing recording, read nothing: a default build pays the
        // level check and no more.
        if span.is_disabled() {
            return Self {
                span,
                task_id: None,
                context_id: None,
            };
        }
        record_agent(&span, peer);
        let message = params.get("message");
        if let Some(id) = message
            .and_then(|m| m.get("messageId"))
            .and_then(|v| v.as_str())
        {
            span.record("a2a.message.id", id);
        }
        let str_at = |v: Option<&serde_json::Value>, key: &str| {
            v.and_then(|v| v.get(key))
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
        };
        Self {
            span,
            // A task-scoped request names its task as `id` (the task
            // methods) or `taskId` (push configuration); a send continuing
            // one names it on the message.
            task_id: if method.contains("PushNotificationConfig") {
                str_at(Some(params), "taskId")
            } else if message.is_some() {
                str_at(message, "taskId")
            } else {
                str_at(Some(params), "id")
            },
            context_id: str_at(message, "contextId"),
        }
    }

    /// Records the call as finished with `result`, the response's JSON:
    /// a task — bare, or as a send's `{"task": …}` — names the task, its
    /// context and its state.
    pub(crate) fn succeeded(mut self, result: Option<&serde_json::Value>) {
        if self.span.is_disabled() {
            return;
        }
        let task = result.map(|r| r.get("task").unwrap_or(r));
        let state = task
            .and_then(|t| t.get("status"))
            .and_then(|s| s.get("state"))
            .and_then(|s| s.as_str());
        if let Some(state) = state {
            // Only a task carries a status; take its id and context too.
            let field = |k: &str| {
                task.and_then(|t| t.get(k))
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
            };
            self.task_id = field("id").or(self.task_id);
            self.context_id = field("contextId").or(self.context_id);
            self.span.record("a2a.task.state", state);
        }
        self.finish();
    }

    /// Records the call as failed with `error`.
    pub(crate) fn failed(self, error: &crate::error::ClientError) {
        if self.span.is_disabled() {
            return;
        }
        self.span.record("error.type", error_type(error).as_str());
        self.span.record("otel.status_code", "ERROR");
        self.finish();
    }

    fn finish(self) {
        if let Some(id) = &self.task_id {
            self.span.record("a2a.task.id", id.as_str());
        }
        if let Some(id) = &self.context_id {
            self.span.record("gen_ai.conversation.id", id.as_str());
        }
    }
}

/// `gen_ai.agent.*` from the card the client was built from — "when
/// available from the Agent Card".
#[cfg(feature = "tracing")]
fn record_agent(span: &tracing::Span, peer: &Peer) {
    if let Some(agent) = &peer.agent {
        let (name, description, version) = &**agent;
        span.record("gen_ai.agent.name", name.as_str());
        if !description.is_empty() {
            span.record("gen_ai.agent.description", description.as_str());
        }
        if !version.is_empty() {
            span.record("gen_ai.agent.version", version.as_str());
        }
    }
}

/// The fully qualified gRPC method — `rpc.method` and the span's name — for
/// one of the client's method names, without allocating.
#[cfg(feature = "tracing")]
fn qualified(method: &str) -> &'static str {
    match method {
        "SendMessage" => "lf.a2a.v1.A2AService/SendMessage",
        "SendStreamingMessage" => "lf.a2a.v1.A2AService/SendStreamingMessage",
        "GetTask" => "lf.a2a.v1.A2AService/GetTask",
        "ListTasks" => "lf.a2a.v1.A2AService/ListTasks",
        "CancelTask" => "lf.a2a.v1.A2AService/CancelTask",
        "SubscribeToTask" => "lf.a2a.v1.A2AService/SubscribeToTask",
        "CreateTaskPushNotificationConfig" => {
            "lf.a2a.v1.A2AService/CreateTaskPushNotificationConfig"
        }
        "GetTaskPushNotificationConfig" => "lf.a2a.v1.A2AService/GetTaskPushNotificationConfig",
        "ListTaskPushNotificationConfigs" => "lf.a2a.v1.A2AService/ListTaskPushNotificationConfigs",
        "DeleteTaskPushNotificationConfig" => {
            "lf.a2a.v1.A2AService/DeleteTaskPushNotificationConfig"
        }
        "GetExtendedAgentCard" => "lf.a2a.v1.A2AService/GetExtendedAgentCard",
        _ => "_OTHER",
    }
}

/// `error.type`: the JSON-RPC error code an agent answered with, the HTTP
/// status, or the failure's class.
#[cfg(feature = "tracing")]
fn error_type(error: &crate::error::ClientError) -> String {
    use crate::error::ClientError as E;
    match error {
        E::Protocol(e) => e.code.as_i32().to_string(),
        E::UnexpectedStatus { status, .. } => status.to_string(),
        E::Timeout(_) => "timeout".to_owned(),
        E::Http(_) | E::HttpClient(_) | E::Transport(_) => "transport".to_owned(),
        E::Serialization(_) => "serialization".to_owned(),
        E::InvalidEndpoint(_) => "invalid_endpoint".to_owned(),
        E::AuthRequired { .. } => "auth_required".to_owned(),
        _ => "_OTHER".to_owned(),
    }
}

/// Makes the call a child of the ambient [`CurrentTrace`](crate::CurrentTrace)
/// when no recorded span is current, then writes its own context as the
/// call's `traceparent`.
#[cfg(feature = "otel")]
pub fn propagate(
    call: &CallSpan,
    peer: &Peer,
    enabled: bool,
    headers: &mut std::collections::HashMap<String, String>,
) {
    use a2a_protocol_types::trace_context::{TRACEPARENT_HEADER, TRACESTATE_HEADER, TraceContext};
    use opentelemetry::trace::TraceContextExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;

    if call.span.is_disabled() {
        return;
    }
    let current_is_recorded = tracing::Span::current()
        .context()
        .span()
        .span_context()
        .is_valid();
    if !current_is_recorded
        && let Some(parent) = crate::CurrentTrace::current()
            .as_ref()
            .and_then(remote_context)
    {
        let _ = call.span.set_parent(parent);
    }
    if !(enabled && peer.carries_headers) {
        return;
    }
    let context = call.span.context();
    let span = context.span();
    let sc = span.span_context();
    if !sc.is_valid() {
        // No OpenTelemetry layer records this span: leave whatever the
        // interceptors wrote.
        return;
    }
    if let Ok(trace) = TraceContext::from_bytes(
        sc.trace_id().to_bytes(),
        sc.span_id().to_bytes(),
        sc.trace_flags().to_u8(),
    ) {
        headers.insert(TRACEPARENT_HEADER.to_owned(), trace.traceparent());
        let state = sc.trace_state().header();
        if state.is_empty() {
            headers.remove(TRACESTATE_HEADER);
        } else {
            headers.insert(TRACESTATE_HEADER.to_owned(), state);
        }
    }
}

#[cfg(feature = "otel")]
fn remote_context(
    trace: &a2a_protocol_types::trace_context::TraceContext,
) -> Option<opentelemetry::Context> {
    use opentelemetry::trace::{
        SpanContext, SpanId, TraceContextExt as _, TraceFlags, TraceId, TraceState,
    };
    let state = trace
        .tracestate()
        .and_then(|s| s.parse::<TraceState>().ok())
        .unwrap_or_default();
    let context = SpanContext::new(
        TraceId::from_hex(trace.trace_id()).ok()?,
        SpanId::from_hex(trace.span_id()).ok()?,
        TraceFlags::new(trace.flags()),
        true,
        state,
    );
    Some(opentelemetry::Context::new().with_remote_span_context(context))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every method the client sends has a qualified name; none falls to
    /// `_OTHER`.
    #[cfg(feature = "tracing")]
    #[test]
    fn every_client_method_has_a_qualified_name() {
        for m in [
            "SendMessage",
            "SendStreamingMessage",
            "GetTask",
            "ListTasks",
            "CancelTask",
            "SubscribeToTask",
            "CreateTaskPushNotificationConfig",
            "GetTaskPushNotificationConfig",
            "ListTaskPushNotificationConfigs",
            "DeleteTaskPushNotificationConfig",
            "GetExtendedAgentCard",
        ] {
            assert_eq!(qualified(m), format!("lf.a2a.v1.A2AService/{m}"));
        }
        assert_eq!(qualified("message/send"), "_OTHER");
    }

    #[test]
    fn peer_reads_system_address_and_default_port() {
        let p = Peer::new("JSONRPC", "https://agents.example.com/a2a", None);
        assert_eq!(p.system, Some("jsonrpc"));
        assert_eq!(p.address.as_deref(), Some("agents.example.com"));
        assert_eq!(p.port, Some(443));
        assert!(p.carries_headers);
        let p = Peer::new("HTTP+JSON", "http://[::1]:8080", None);
        assert_eq!(p.system, Some("a2a_http_json"));
        assert_eq!(p.address.as_deref(), Some("::1"));
        assert_eq!(p.port, Some(8080));
        let p = Peer::new("SOMETHING", "not a url", None);
        assert_eq!(
            (p.system, p.address, p.port, p.carries_headers),
            (None, None, None, false)
        );
    }
}
