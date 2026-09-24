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
//! the features: a metrics callback is always compiled. A failed call carries
//! the status code its binding put on the wire — the JSON-RPC error code, the
//! HTTP status, the gRPC status name — as both `rpc.status_code` and
//! `error.type`, as the semantic conventions ask ("If a status code is
//! returned and it indicates an error, `error.type` SHOULD be set to that
//! status code"). Each binding's own mapping produces it, so the metric
//! cannot disagree with the response.
//!
//! Attribute names and rules are the OpenTelemetry semantic conventions'
//! `docs/rpc/{rpc-spans,rpc-metrics,grpc,json-rpc}.md`, read upstream on
//! 2026-09-23.

use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

mod names;
#[cfg(feature = "otel")]
mod otel;

#[cfg(feature = "tracing")]
use crate::handler::InboundTracePolicy;
use crate::handler::RequestHandler;
use crate::metrics::{Metrics, RpcCall};
#[cfg(feature = "tracing")]
use names::OTHER;
pub use names::{RpcSystem, WireStatus, a2a_method};
#[cfg(feature = "otel")]
pub use otel::current_recorded_span;
#[cfg(feature = "otel")]
use otel::remote_parent;

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
                let span = tracing::info_span!(
                    target: "a2a_protocol_server::rpc",
                    "a2a.rpc",
                    otel.name = name,
                    otel.kind = "server",
                    otel.status_code = tracing::field::Empty,
                    rpc.system.name = system.name(),
                    rpc.method = qualified,
                    rpc.method_original = tracing::field::Empty,
                    rpc.status_code = tracing::field::Empty,
                    error.type = tracing::field::Empty,
                    http.request.method = tracing::field::Empty,
                    http.route = tracing::field::Empty,
                );
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

    /// Records a finished call on the span and in the metrics.
    fn finish(&self, duration: Duration, failed_with: Option<&str>) {
        // gRPC always has a status; the other bindings have one only on error.
        let status_code = match (failed_with, self.system) {
            (Some(code), _) => Some(code),
            (None, RpcSystem::Grpc) => Some("OK"),
            (None, _) => None,
        };
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

/// The future inside a call's span, as the call's future is wrapped when
/// tracing is compiled in.
#[cfg(feature = "tracing")]
type Inner<F> = tracing::instrument::Instrumented<tokio::task::futures::TaskLocalFuture<bool, F>>;
#[cfg(not(feature = "tracing"))]
type Inner<F> = F;

pin_project_lite::pin_project! {
    /// A call running in its span; when it finishes, it is recorded and its
    /// outcome handed on. See [`ServerSpan::run`].
    pub struct Call<F, C, T>
    where
        F: Future,
    {
        #[pin]
        inner: Inner<F>,
        pending: Pending,
        finish: Option<(C, T)>,
    }
}

impl<F, C, T, R> Future for Call<F, C, T>
where
    F: Future,
    C: FnOnce(&F::Output, RpcSystem) -> Option<Cow<'static, str>>,
    T: FnOnce(F::Output) -> R,
{
    type Output = R;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<R> {
        let this = self.project();
        let out = std::task::ready!(this.inner.poll(cx));
        // Both are taken only here, on the poll that returns `Ready`, so they
        // are missing only if the future is polled again after completing —
        // which `Future`'s contract lets an implementation panic on, and
        // which every `async` block does. No caller in this crate re-polls.
        let (classify, then) = this.finish.take().expect("a call polled after it finished");
        let call = this
            .pending
            .call
            .take()
            .expect("a call polled after it finished");
        let status = classify(&out, call.system);
        call.finish(this.pending.started.elapsed(), status.as_deref());
        call.enter(|| then(out)).into()
    }
}

/// A call in flight; records it as cancelled if dropped unfinished.
struct Pending {
    call: Option<ServerSpan>,
    started: Instant,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(call) = self.call.take() {
            call.finish(self.started.elapsed(), Some(CANCELLED));
        }
    }
}

/// Records a request refused before it named a method — a body that is not
/// JSON-RPC, a batch over the limit, an unsupported content type — as a
/// failed call without `rpc.method`, which the conventions require only "if
/// available". No span: there is no method to name one for, and the peer's
/// trace context has not been read.
pub fn record_unrouted(
    handler: &RequestHandler,
    system: RpcSystem,
    started: Instant,
    status: &str,
) {
    handler.metrics.on_rpc_call(&RpcCall {
        system: system.name(),
        method: None,
        duration: started.elapsed(),
        status_code: Some(status),
        error_type: Some(status),
    });
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
pub(crate) fn in_current_span<F: Future>(fut: F) -> impl Future<Output = F::Output> {
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

/// Runs the executor for one task in its child span, which carries the task
/// and context ids — so its log events, and a user's own events emitted from
/// inside the executor, can be followed by either id (the claim
/// `deployment/observability.md` makes; audit O1).
pub fn in_executor_span<F: Future>(
    task_id: &str,
    context_id: &str,
    fut: F,
) -> impl Future<Output = F::Output> + use<F> {
    #[cfg(feature = "tracing")]
    {
        let untraced = untraced();
        let span = if untraced {
            tracing::Span::none()
        } else {
            tracing::info_span!(
                target: "a2a_protocol_server::rpc",
                "a2a.task",
                otel.name = "a2a.execute",
                a2a.task.id = task_id,
                a2a.context.id = context_id,
            )
        };
        tracing::Instrument::instrument(UNTRACED.scope(untraced, fut), span)
    }
    #[cfg(not(feature = "tracing"))]
    {
        let _ = (task_id, context_id);
        fut
    }
}

#[cfg(test)]
mod tests;
