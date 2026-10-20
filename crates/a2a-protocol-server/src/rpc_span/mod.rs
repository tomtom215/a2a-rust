// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The span each inbound A2A call runs in, and the `rpc.server.call.duration`
//! it records (ADR 0013; audit O1, O2, O4, O5, O11).
//!
//! Every binding opens one `SERVER` span per A2A call, named for the
//! fully-qualified method the gRPC binding serves —
//! `lf.a2a.v1.A2AService/{Method}` — with `rpc.system.name` saying which
//! binding it came in on. One span shape per call, whatever the binding, is
//! what makes a cross-binding, cross-language trace comparable.
//!
//! The span is a `tracing` span. With the `otel` feature and
//! `tracing-opentelemetry` installed it is exported, and the caller's
//! `traceparent` — subject to the handler's
//! [`InboundTracePolicy`](crate::handler::InboundTracePolicy) — becomes
//! its remote parent; the `traceparent` this server then sends downstream
//! names this span, which the exporter has, rather than an id minted for the
//! purpose that nothing records (see `build_call_context`). Without the
//! `tracing` feature every span here is compiled out and a call runs as a
//! plain future.
//!
//! The call's duration and outcome go to [`Metrics::on_rpc_call`] whatever
//! the features: a metrics callback is always compiled. A call that did not
//! succeed carries the status code its binding put on the wire — the
//! JSON-RPC error code, the HTTP status, the gRPC status name — as
//! `rpc.status_code`, and, when that status "indicates an error", also as
//! `error.type` with `Error` status, as the semantic conventions ask. Which
//! statuses indicate one is each binding's conventions' rule
//! ([`RpcSystem::is_server_error`]): every JSON-RPC code, a 5xx but not a
//! 4xx, and gRPC's six server-side codes but not `NOT_FOUND`. Each binding's
//! own mapping produces the status, so the metric cannot disagree with the
//! response.
//!
//! Attribute names and rules are the OpenTelemetry semantic conventions'
//! `docs/rpc/{rpc-spans,rpc-metrics,grpc,json-rpc}.md`, read upstream on
//! 2026-09-23; `rpc-spans.md` and `rpc-metrics.md` re-read on `main` on
//! 2026-10-18, with every attribute name, the span-name rule and the
//! duration buckets used here unchanged.

use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "tracing")]
mod a2a;
mod agent;
mod binding;
mod call;
mod names;
#[cfg(feature = "otel")]
mod otel;
mod refusal;
#[cfg(feature = "tool-loop")]
mod tool;
mod webhook;

#[cfg(feature = "tracing")]
use crate::handler::InboundTracePolicy;
use crate::handler::RequestHandler;
use crate::metrics::{Metrics, RpcCall};
#[cfg(feature = "tracing")]
pub use a2a::{
    record_message, record_task, record_task_response, record_task_state, record_tenant,
};
#[cfg(feature = "tracing")]
pub use agent::OutputRecorder;
pub use agent::{ExecutorSpan, SpanSettings};
pub use binding::{BindingSpan, CustomBinding};
use call::{Call, Pending};
#[cfg(feature = "tracing")]
use names::OTHER;
pub use names::{RpcSystem, WireStatus, a2a_method};
#[cfg(feature = "otel")]
pub use otel::current_recorded_span;
#[cfg(feature = "otel")]
use otel::remote_parent;
#[cfg(feature = "grpc")]
pub use refusal::record_refused;
pub use refusal::{record_if_refused, record_unrouted, refused};
#[cfg(feature = "tool-loop")]
pub use tool::GenAiSpan;
pub use webhook::WebhookSpan;

/// The longest `rpc.method_original` recorded. The conventions ask for the
/// original value; a request body may be megabytes, and an attribute that
/// size is a cost the peer chooses. 128 bytes holds any name a real client
/// sends.
#[cfg(feature = "tracing")]
const METHOD_ORIGINAL_MAX: usize = 128;

/// The `error.type` of a call whose future was dropped before it finished —
/// the peer went away, or gave up waiting. A component-specific value, as
/// the conventions allow where no status code was sent.
const CANCELLED: &str = "cancelled";

/// One inbound call: its span, its clock, and where its duration goes.
pub struct ServerSpan {
    system: RpcSystem,
    method: &'static str,
    metrics: Arc<dyn Metrics>,
    #[cfg(feature = "tracing")]
    span: tracing::Span,
    /// The handler's policy is [`InboundTracePolicy::Drop`]: this call, and
    /// everything spawned for it, records no span.
    #[cfg(feature = "tracing")]
    untraced: bool,
}

#[cfg(feature = "tracing")]
tokio::task_local! {
    /// Set while a call under [`InboundTracePolicy::Drop`] runs, and carried
    /// into the tasks it spawns, so their spans are not recorded either: the
    /// policy promises "nothing is recorded", and a child span with no
    /// parent would be recorded as the root of a trace of its own.
    static UNTRACED: bool;
}

/// Whether the code running now belongs to a call that records no spans.
#[cfg(feature = "tracing")]
fn untraced() -> bool {
    UNTRACED.try_with(|u| *u).unwrap_or(false)
}

impl ServerSpan {
    /// Opens the `SERVER` span for one inbound call.
    ///
    /// Under [`InboundTracePolicy::Drop`] no span is recorded, for the call
    /// or for anything spawned for it, as that policy promises — the call's
    /// metric still is. Under [`Continue`](InboundTracePolicy::Continue) a
    /// well-formed `traceparent` is the span's remote parent. Under
    /// [`Restart`](InboundTracePolicy::Restart), or without one, the peer's
    /// trace is ignored: the parent is whatever span is current in this
    /// process — a new root when there is none.
    #[cfg_attr(not(feature = "otel"), allow(unused_variables))]
    pub fn open(
        handler: &RequestHandler,
        system: RpcSystem,
        method: &str,
        headers: Option<&HashMap<String, String>>,
    ) -> Self {
        let qualified = a2a_method(method);
        #[cfg(feature = "tracing")]
        let span = {
            let policy = handler.inbound_trace_policy;
            if policy == InboundTracePolicy::Drop {
                tracing::Span::none()
            } else {
                // Named `{rpc.method}`, or `{rpc.system.name}` for `_OTHER`.
                let name = if qualified == OTHER {
                    system.name()
                } else {
                    qualified
                };
                let span = call_span(name, system, qualified);
                a2a::record_agent(&span, handler.agent_card.as_ref());
                if qualified == OTHER {
                    span.record(
                        "rpc.method_original",
                        truncated(method, METHOD_ORIGINAL_MAX),
                    );
                }
                #[cfg(feature = "otel")]
                if policy == InboundTracePolicy::Continue
                    && let Some(parent) = headers.and_then(remote_parent)
                {
                    use tracing_opentelemetry::OpenTelemetrySpanExt as _;
                    // Refused only when the span is already entered or has no
                    // OpenTelemetry layer; neither is an error in the call.
                    let _ = span.set_parent(parent);
                }
                span
            }
        };
        Self {
            system,
            method: qualified,
            metrics: Arc::clone(&handler.metrics),
            #[cfg(feature = "tracing")]
            untraced: handler.inbound_trace_policy == InboundTracePolicy::Drop,
            #[cfg(feature = "tracing")]
            span,
        }
    }

    /// [`open`](Self::open) for a JSON-RPC request — over HTTP or WebSocket —
    /// adding what `docs/rpc/json-rpc.md` asks of its span:
    /// `jsonrpc.protocol.version` `2.0` (Conditionally Required "if other
    /// than the default version (`1.0`)"); `rpc.method_original`, the method
    /// as sent (Conditionally Required "if and only if it's different than
    /// `rpc.method`", which a served method's `SendMessage` always is); and
    /// `jsonrpc.request.id` (Recommended; not for a notification or a `null`
    /// id, which the conventions say not to capture). The two the peer
    /// chooses are cut to 128 bytes.
    pub fn open_jsonrpc(
        handler: &RequestHandler,
        request: &a2a_protocol_types::jsonrpc::JsonRpcRequest,
        headers: Option<&HashMap<String, String>>,
    ) -> Self {
        let call = Self::open(handler, RpcSystem::JsonRpc, &request.method, headers);
        #[cfg(feature = "tracing")]
        {
            call.span.record("jsonrpc.protocol.version", "2.0");
            // `open` records it for `_OTHER`; a field recorded twice is
            // exported twice (`a2a.rs`).
            if call.method != OTHER {
                call.span.record(
                    "rpc.method_original",
                    truncated(&request.method, METHOD_ORIGINAL_MAX),
                );
            }
            match request.id.as_value() {
                Some(serde_json::Value::String(id)) => {
                    call.span
                        .record("jsonrpc.request.id", truncated(id, METHOD_ORIGINAL_MAX));
                }
                Some(id) => {
                    let id = id.to_string();
                    call.span
                        .record("jsonrpc.request.id", truncated(&id, METHOD_ORIGINAL_MAX));
                }
                None => {}
            }
        }
        call
    }

    /// Adds the HTTP request method and matched route template to an
    /// HTTP+JSON call's span, so HTTP-oriented tooling still finds it (ADR
    /// 0013, option 2). `route` is the template (`/tasks/{id}`), never the
    /// path: a path carries ids, and a span attribute is no place for them.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::missing_const_for_fn)
    )]
    pub fn with_http(self, method: &str, route: &str) -> Self {
        #[cfg(feature = "tracing")]
        {
            self.span.record("http.request.method", method);
            self.span.record("http.route", route);
        }
        self
    }

    /// Runs `fut` inside this span, then records the call: its duration, and
    /// for an `Err`, the status code the binding sends for it.
    ///
    /// If the returned future is dropped first — the peer disconnected, or
    /// gave up — the call is recorded as failed with `error.type`
    /// `cancelled`, so a hung executor behind a client timeout still shows.
    pub fn run<T, E, F>(self, fut: F) -> impl Future<Output = Result<T, E>>
    where
        E: WireStatus,
        F: Future<Output = Result<T, E>>,
    {
        self.run_with(fut, |out| out)
    }

    /// [`run`](Self::run), then `then` on the outcome, still inside the span:
    /// for a binding that builds its response from the outcome and spawns
    /// work doing so — an SSE response's writer task — so that work is a
    /// child of the call rather than the root of a trace of its own. The
    /// call's recorded duration ends before `then`.
    pub fn run_with<T, E, F, R>(
        self,
        fut: F,
        then: impl FnOnce(Result<T, E>) -> R,
    ) -> impl Future<Output = R>
    where
        E: WireStatus,
        F: Future<Output = Result<T, E>>,
    {
        self.call(
            fut,
            |out, system| out.as_ref().err().map(|e| e.wire_status(system)),
            then,
        )
    }

    /// [`run`](Self::run) for a binding whose response already says how the
    /// call went: an HTTP+JSON response's status *is* its wire status, so
    /// timing the whole route — parsing the body and params included — and
    /// classifying by that status records a request refused before the
    /// handler ran exactly as a caller saw it.
    pub fn run_response<B, F>(self, fut: F) -> impl Future<Output = hyper::Response<B>>
    where
        F: Future<Output = hyper::Response<B>>,
    {
        self.call(
            fut,
            |resp, _| {
                (resp.status().as_u16() >= 400)
                    .then(|| Cow::Owned(resp.status().as_u16().to_string()))
            },
            |resp| resp,
        )
    }

    /// The one implementation behind the three above: `classify` names the
    /// status of a failed outcome, `None` for success.
    ///
    /// Returns a [`Call`], which holds `fut` once. An `async` block awaiting
    /// it held it twice — as the captured argument and again where it was
    /// awaited — which doubled every dispatcher's future past clippy's
    /// `large_futures` bound; boxing it instead cost an allocation and a copy
    /// of up to 84 KB per call, measured at +11% on an HTTP+JSON send.
    fn call<F, C, T, R>(self, fut: F, classify: C, then: T) -> Call<F, C, T>
    where
        F: Future,
        C: FnOnce(&F::Output, RpcSystem) -> Option<Cow<'static, str>>,
        T: FnOnce(F::Output) -> R,
    {
        #[cfg(feature = "tracing")]
        let inner =
            tracing::Instrument::instrument(UNTRACED.scope(self.untraced, fut), self.span.clone());
        #[cfg(not(feature = "tracing"))]
        let inner = fut;
        Call {
            inner,
            pending: Pending {
                call: Some(self),
                started: Instant::now(),
            },
            finish: Some((classify, then)),
        }
    }

    /// Runs `then` inside this call's span, and inside its trace policy.
    #[cfg_attr(not(feature = "tracing"), allow(clippy::unused_self))]
    fn enter<R>(&self, then: impl FnOnce() -> R) -> R {
        #[cfg(feature = "tracing")]
        {
            self.span
                .in_scope(|| UNTRACED.sync_scope(self.untraced, then))
        }
        #[cfg(not(feature = "tracing"))]
        {
            then()
        }
    }

    /// Records a finished call on the span and in the metrics: `status` is
    /// the wire status of a call that did not succeed, `None` for one that
    /// did. Whether it is an error is the binding's conventions' call
    /// ([`RpcSystem::is_server_error`]); a call that never finished is one
    /// on every binding.
    fn finish(&self, duration: Duration, status: Option<&str>) {
        // gRPC always has a status; the other bindings have one only on error.
        let status_code = match (status, self.system) {
            (Some(code), _) => Some(code),
            (None, RpcSystem::Grpc) => Some("OK"),
            (None, RpcSystem::Custom(binding)) => binding.ok_status,
            (None, _) => None,
        };
        let failed_with = status.filter(|s| *s == CANCELLED || self.system.is_server_error(s));
        #[cfg(feature = "tracing")]
        {
            if let Some(code) = status_code {
                self.span.record("rpc.status_code", code);
            }
            if let Some(code) = failed_with {
                self.span.record("error.type", code);
                self.span.record("otel.status_code", "ERROR");
            }
        }
        self.metrics.on_rpc_call(&RpcCall {
            system: self.system.name(),
            method: Some(self.method),
            duration,
            status_code,
            error_type: failed_with,
        });
    }
}

impl std::fmt::Debug for ServerSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerSpan")
            .field("system", &self.system)
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

/// The `SERVER` span for one call, every field declared up front: `tracing`
/// records only fields a span declared when it opened.
#[cfg(feature = "tracing")]
fn call_span(name: &str, system: RpcSystem, qualified: &'static str) -> tracing::Span {
    tracing::info_span!(
        target: "a2a_protocol_server::rpc",
        "a2a.rpc",
        otel.name = name,
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        rpc.system.name = system.name(),
        rpc.method = qualified,
        rpc.method_original = tracing::field::Empty,
        rpc.status_code = tracing::field::Empty,
        jsonrpc.protocol.version = tracing::field::Empty,
        jsonrpc.request.id = tracing::field::Empty,
        error.type = tracing::field::Empty,
        http.request.method = tracing::field::Empty,
        http.route = tracing::field::Empty,
        a2a.method.name = a2a::method_name(qualified),
        a2a.protocol.version = a2a_protocol_types::A2A_VERSION,
        a2a.tenant = tracing::field::Empty,
        a2a.message.id = tracing::field::Empty,
        a2a.task.id = tracing::field::Empty,
        a2a.task.state = tracing::field::Empty,
        gen_ai.conversation.id = tracing::field::Empty,
        gen_ai.agent.name = tracing::field::Empty,
        gen_ai.agent.description = tracing::field::Empty,
        gen_ai.agent.version = tracing::field::Empty,
    )
}

/// `s` cut to at most `max` bytes, on a character boundary.
#[cfg(feature = "tracing")]
fn truncated(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Runs a task spawned on behalf of the current call — the executor, the
/// event processor, a push delivery — in an `INTERNAL` child span of it, so
/// the work shows up under the call in a trace and its log events carry the
/// call's context across the `tokio::spawn` (audit O4).
pub fn in_child_span<F: Future>(name: &'static str, fut: F) -> impl Future<Output = F::Output> {
    #[cfg(feature = "tracing")]
    {
        let untraced = untraced();
        let span = if untraced {
            tracing::Span::none()
        } else {
            tracing::info_span!(target: "a2a_protocol_server::rpc", "a2a.task", otel.name = name)
        };
        tracing::Instrument::instrument(UNTRACED.scope(untraced, fut), span)
    }
    #[cfg(not(feature = "tracing"))]
    {
        let _ = name;
        fut
    }
}

/// Runs work spawned to finish a call's own job — not a task of its own —
/// in the call's span, carrying whether the call is untraced across the
/// `tokio::spawn`. For work that must outlive the request future when its
/// client goes away (N27) without adding a span to the call's trace.
pub fn in_current_span<F: Future>(fut: F) -> impl Future<Output = F::Output> {
    #[cfg(feature = "tracing")]
    {
        let untraced = untraced();
        tracing::Instrument::instrument(UNTRACED.scope(untraced, fut), tracing::Span::current())
    }
    #[cfg(not(feature = "tracing"))]
    {
        fut
    }
}

#[cfg(test)]
mod jsonrpc_tests;
#[cfg(test)]
mod span_tests;
#[cfg(test)]
mod tests;
