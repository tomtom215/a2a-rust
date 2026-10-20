// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The per-call span and metric for a binding implemented outside this
//! crate, such as `a2a-protocol-slimrpc` (ADR 0013, decision 2: "an RPC
//! `SERVER` span per A2A method on every binding").
//!
//! The bindings in this crate open a [`ServerSpan`] directly. It stays
//! private, with the per-binding status mapping beside it, so what is public
//! is only what a binding outside needs: its own `rpc.system.name`, the
//! status it sends for each [`ServerError`], and the call to run.

use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;

use super::{RpcSystem, ServerSpan};
use crate::RequestHandler;
use crate::error::{ServerError, ServerResult};

/// How a binding outside this crate names itself on its spans and in
/// `rpc.server.call.duration`.
#[derive(Clone, Copy)]
pub struct CustomBinding {
    pub(super) name: &'static str,
    pub(super) ok_status: Option<&'static str>,
    pub(super) error_status: fn(&ServerError) -> Cow<'static, str>,
    pub(super) server_error: fn(&str) -> bool,
}

/// A custom binding's default rule: every failure is a server error.
const fn every_status(_: &str) -> bool {
    true
}

impl CustomBinding {
    /// A binding whose `rpc.system.name` is `name` and which sends
    /// `error_status(e)` as the status of a call that failed with `e`.
    ///
    /// `name` should be the binding's own name in lower case
    /// (`"slimrpc"`): the semantic conventions name only a few RPC systems
    /// and let others use "a custom value". `error_status` is recorded as
    /// `rpc.status_code` and `error.type`, so it should be the binding's
    /// error mapping itself rather than a second copy of it, or the metric
    /// can disagree with what the peer received.
    #[must_use]
    pub const fn new(
        name: &'static str,
        error_status: fn(&ServerError) -> Cow<'static, str>,
    ) -> Self {
        Self {
            name,
            ok_status: None,
            error_status,
            server_error: every_status,
        }
    }

    /// Counts a failure as an error on the span and in the metric — sets
    /// `error.type` and `Error` status — only for gRPC's server-side
    /// statuses (`UNKNOWN`, `DEADLINE_EXCEEDED`, `UNIMPLEMENTED`, `INTERNAL`,
    /// `UNAVAILABLE`, `DATA_LOSS`), as the semantic conventions' gRPC server
    /// span does, for a binding whose `error_status` returns gRPC status
    /// names. Any other status is still recorded as `rpc.status_code`.
    /// Without it every failure is an error, as before this existed.
    #[must_use]
    pub const fn with_grpc_server_errors(mut self) -> Self {
        self.server_error = super::names::grpc_server_error;
        self
    }

    /// The status a successful call sends, for a binding that sends one —
    /// `"OK"` for one with gRPC's status codes. Without it a successful
    /// call records no `rpc.status_code`, as on JSON-RPC.
    #[must_use]
    pub const fn with_ok_status(mut self, status: &'static str) -> Self {
        self.ok_status = Some(status);
        self
    }
}

impl std::fmt::Debug for CustomBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomBinding")
            .field("name", &self.name)
            .field("ok_status", &self.ok_status)
            .finish_non_exhaustive()
    }
}

/// One inbound call on a binding outside this crate: the `SERVER` span it
/// runs in, and its `rpc.server.call.duration`, exactly as the bindings in
/// this crate record theirs.
///
/// The span is named `lf.a2a.v1.A2AService/{Method}` and carries the A2A
/// attributes; the handler's
/// [`InboundTracePolicy`](crate::InboundTracePolicy) decides whether a
/// `traceparent` among the call's headers becomes its parent; the duration
/// goes to the handler's [`Metrics`](crate::Metrics), failed calls with the
/// binding's own status. Open it after the request has decoded, and run the
/// `RequestHandler` call inside it:
///
/// ```rust,no_run
/// # use std::borrow::Cow;
/// # use std::collections::HashMap;
/// # use a2a_protocol_server::{BindingSpan, CustomBinding, RequestHandler, ServerError};
/// # use a2a_protocol_types::params::TaskQueryParams;
/// const MY_BINDING: CustomBinding =
///     CustomBinding::new("mybinding", |e: &ServerError| {
///         Cow::Owned(e.to_a2a_error().code.as_i32().to_string())
///     });
///
/// # async fn call(handler: &RequestHandler, params: TaskQueryParams, headers: HashMap<String, String>) {
/// let task = BindingSpan::open(handler, MY_BINDING, "GetTask", Some(&headers))
///     .run(handler.on_get_task(params, Some(&headers)))
///     .await;
/// # }
/// ```
#[derive(Debug)]
pub struct BindingSpan(ServerSpan);

impl BindingSpan {
    /// Opens the span for one call of `method` — the A2A method as the gRPC
    /// service spells it (`"GetTask"`); one this crate does not know is
    /// recorded as `_OTHER`, so a peer's string never becomes a span name or
    /// a metric attribute.
    #[must_use]
    pub fn open(
        handler: &RequestHandler,
        binding: CustomBinding,
        method: &str,
        headers: Option<&HashMap<String, String>>,
    ) -> Self {
        Self(ServerSpan::open(
            handler,
            RpcSystem::Custom(binding),
            method,
            headers,
        ))
    }

    /// Runs `fut` inside the span, then records the call: its duration and,
    /// for an `Err`, the binding's status for it. A call whose future is
    /// dropped first is recorded as failed with `error.type` `cancelled`.
    pub fn run<T, F>(self, fut: F) -> impl Future<Output = ServerResult<T>>
    where
        F: Future<Output = ServerResult<T>>,
    {
        self.0.run(fut)
    }

    /// [`run`](Self::run), then `then` on the outcome, inside the call: for
    /// a call that answers with a stream. A stream `then` sets up claims the
    /// call with [`StreamEnd::claim`](crate::StreamEnd::claim); a successful
    /// call is then recorded when the stream ends, with the
    /// [`StreamOutcome`](crate::StreamOutcome) passed to
    /// [`StreamEnd::end`](crate::StreamEnd::end) — or as `cancelled` if the
    /// claim is dropped first — so its span and duration cover the stream,
    /// as the semantic conventions ask of a streaming call.
    pub fn run_with<T, F, R>(
        self,
        fut: F,
        then: impl FnOnce(ServerResult<T>) -> R,
    ) -> impl Future<Output = R>
    where
        F: Future<Output = ServerResult<T>>,
    {
        self.0.run_with(fut, then)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{Metrics, RpcCall};

    /// System, method, status code and error type of each call.
    type Calls = Arc<Mutex<Vec<[Option<String>; 4]>>>;

    struct Recording(Calls);

    impl Metrics for Recording {
        fn on_rpc_call(&self, call: &RpcCall<'_>) {
            self.0.lock().unwrap().push([
                Some(call.system.to_owned()),
                call.method.map(str::to_owned),
                call.status_code.map(str::to_owned),
                call.error_type.map(str::to_owned),
            ]);
        }
    }

    fn handler(calls: &Calls) -> RequestHandler {
        struct Idle;
        crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
        crate::RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_metrics(Recording(Arc::clone(calls)))
            .build()
            .unwrap()
    }

    const CODES: CustomBinding = CustomBinding::new("codes", |e| {
        Cow::Owned(format!("E{}", e.to_a2a_error().code.as_i32()))
    });

    fn strings<const N: usize>(row: [Option<&str>; N]) -> [Option<String>; N] {
        row.map(|s| s.map(str::to_owned))
    }

    /// A call is recorded under the binding's own name, a failure with the
    /// binding's own status, and a success with no status unless the
    /// binding sends one.
    #[tokio::test]
    async fn calls_are_recorded_with_the_bindings_name_and_statuses() {
        let calls = Calls::default();
        let handler = handler(&calls);
        let _ = BindingSpan::open(&handler, CODES, "GetTask", None)
            .run(async { Err::<(), _>(ServerError::TaskNotFound("t".into())) })
            .await;
        let _ = BindingSpan::open(&handler, CODES, "ListTasks", None)
            .run(async { Ok::<_, ServerError>(()) })
            .await;
        let _ = BindingSpan::open(&handler, CODES.with_ok_status("FINE"), "Nonsense", None)
            .run(async { Ok::<_, ServerError>(()) })
            .await;
        assert_eq!(
            *calls.lock().unwrap(),
            [
                strings([
                    Some("codes"),
                    Some("lf.a2a.v1.A2AService/GetTask"),
                    Some("E-32001"),
                    Some("E-32001")
                ]),
                strings([
                    Some("codes"),
                    Some("lf.a2a.v1.A2AService/ListTasks"),
                    None,
                    None
                ]),
                strings([Some("codes"), Some("_OTHER"), Some("FINE"), None]),
            ]
        );
    }

    #[test]
    fn debug_shows_the_name_and_ok_status() {
        let shown = format!("{:?}", CODES.with_ok_status("OK"));
        assert!(
            shown.contains("\"codes\"") && shown.contains("\"OK\""),
            "{shown}"
        );
    }
}
