// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One call in flight: its span, its clock and its
//! `rpc.client.call.duration`, from the moment the span opens until the call
//! has a typed result — or the caller drops it.
//!
//! **Ending after validation.** A call is recorded as it ended for the
//! caller, not as the agent answered. A response the client then refuses
//! (an interceptor's `after` hook errors, the result does not decode,
//! `received` finds it malformed, a content signature does not verify) is a
//! failed call, so the span and the metric were wrong while they ended at the
//! transport: a client whose agent returned garbage showed a flawless
//! success rate.
//!
//! **Dropped calls.** A `tokio::time::timeout` or a `select!` drops the
//! future of a call still waiting for its agent. Nothing after the `.await`
//! runs then, so the span ended with no status and the metric held no point:
//! the calls a caller gave up on, which are the slow ones, were the calls the
//! telemetry could not see. The server records the same event, a call its
//! peer abandoned, as `error.type` `cancelled` (`rpc_span`), and so does
//! this. The interceptors' error hook does not run for such a call: it is
//! async and a destructor cannot await.
//!
//! **Not an RPC.** A call refused before anything is sent — a capability the
//! card does not declare, a message that is not well formed, a `before` hook
//! that fails (a token provider that cannot get a token) — never opens a
//! span: nothing was called, and the conventions' client metric is the
//! duration of an outgoing RPC.
//!
//! Without the `tracing` feature there is no span and no metric, and
//! [`InFlight`] does nothing.

use std::collections::HashMap;
use std::future::Future;

use crate::A2aClient;
use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;

#[cfg(feature = "tracing")]
use std::time::Instant;

#[cfg(feature = "tracing")]
use super::{CallSpan, Status};
#[cfg(feature = "tracing")]
use crate::streaming::event_stream::call::StreamCall;

/// A call in flight, until [`end`](Self::end) or [`attach`](Self::attach);
/// dropped before either, it records the call as cancelled.
///
/// One type whatever the features, its bodies saying what `tracing` and
/// `otel` add: a second type for a build without them would be code no test
/// of the full build compiles, so no mutation of it could be caught.
#[cfg_attr(not(feature = "otel"), allow(dead_code))]
pub struct InFlight<'a> {
    #[cfg(feature = "tracing")]
    client: &'a A2aClient,
    #[cfg(feature = "tracing")]
    method: &'a str,
    #[cfg(feature = "tracing")]
    call: CallSpan,
    #[cfg(feature = "tracing")]
    started: Instant,
    #[cfg(feature = "tracing")]
    ended: bool,
    #[cfg(not(feature = "tracing"))]
    _borrowed: std::marker::PhantomData<&'a ()>,
}

// Without `tracing`, and for `propagate` without `otel`, these do nothing and
// take what they do not use.
#[cfg_attr(
    not(feature = "otel"),
    allow(
        clippy::unused_self,
        clippy::needless_pass_by_value,
        clippy::needless_pass_by_ref_mut,
        clippy::missing_const_for_fn,
        unused_mut,
        unused_variables
    )
)]
impl<'a> InFlight<'a> {
    /// Opens the span of `method`'s call to `client`'s agent, and starts its
    /// clock.
    pub fn open(client: &'a A2aClient, method: &'a str, params: &serde_json::Value) -> Self {
        #[cfg(feature = "tracing")]
        {
            let started = Instant::now();
            Self {
                client,
                method,
                call: CallSpan::open(method, &client.peer, params),
                started,
                ended: false,
            }
        }
        #[cfg(not(feature = "tracing"))]
        {
            Self {
                _borrowed: std::marker::PhantomData,
            }
        }
    }

    /// Writes this call's trace context into `headers` (`call_span::propagate`).
    pub fn propagate(&self, headers: &mut HashMap<String, String>) {
        #[cfg(feature = "otel")]
        super::propagate(
            &self.call,
            &self.client.peer,
            self.client.config.propagate_trace,
            headers,
        );
    }

    /// Awaits `send` inside the call's span, and notes the JSON-RPC id the
    /// transport sent for the span's `jsonrpc.request.id`.
    pub async fn send<T>(&self, send: impl Future<Output = ClientResult<T>>) -> ClientResult<T> {
        #[cfg(feature = "tracing")]
        {
            let send = tracing::Instrument::instrument(send, self.call.span.clone());
            let (result, request_id) = super::noting_request_id(send).await;
            self.call.request_id(request_id.as_deref());
            result
        }
        #[cfg(not(feature = "tracing"))]
        {
            send.await
        }
    }

    /// The agent answered with `result`: records what it says of its task.
    pub fn response(&mut self, result: &serde_json::Value) {
        #[cfg(feature = "tracing")]
        self.call.response(Some(result));
    }

    /// The call is over: it failed with `error`, or succeeded.
    pub fn end(mut self, error: Option<&ClientError>) {
        #[cfg(feature = "tracing")]
        {
            let system = self.client.peer.system;
            let status = error.map_or_else(|| Status::ok(system), |e| Status::failed(system, e));
            self.record(&status);
        }
    }

    /// The call opened `stream`, which carries it from here: its span stays
    /// open and its duration runs until the stream is dropped.
    pub fn attach(mut self, stream: EventStream) -> EventStream {
        #[cfg(feature = "tracing")]
        {
            self.ended = true;
            let call = StreamCall::new(self.call.span.clone(), self.call.system);
            #[cfg(feature = "otel")]
            let call = call.measured(
                self.client.metric.clone(),
                self.client.peer.clone(),
                self.method,
                self.started,
            );
            self.call.finish();
            stream.with_call(call)
        }
        #[cfg(not(feature = "tracing"))]
        {
            stream
        }
    }

    #[cfg(feature = "tracing")]
    fn record(&mut self, status: &Status) {
        self.ended = true;
        self.call.end(status);
        #[cfg(feature = "otel")]
        self.client.metric.record(
            &self.client.peer,
            super::qualified(self.method),
            self.started,
            status,
        );
    }
}

#[cfg(feature = "tracing")]
impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.record(&Status::cancelled());
        }
    }
}

#[cfg(test)]
mod tests;
