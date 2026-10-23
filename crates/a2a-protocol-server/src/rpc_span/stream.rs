// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A streaming call ends when its stream does.
//!
//! `docs/rpc/rpc-spans.md` (semantic conventions at `55c98cf`): "For
//! streaming RPCs, the span SHOULD cover the full lifetime of the request
//! and/or response streams until they are closed or terminated", and
//! `rpc-metrics.md` asks that `rpc.server.call.duration` "be the same as the
//! RPC server span duration". A `SendStreamingMessage` or `SubscribeToTask`
//! handler returns as soon as the stream is open, so a call recorded when
//! its handler returned ended at the handshake, reported a stream that later
//! failed as a success, and never showed a client that went away.
//!
//! So the code that forwards a stream to the wire claims the call it runs
//! for ([`StreamEnd::claim`], [`spanning_stream`]) while it is being set up
//! inside that call. The call, when its handler returns successfully, hands
//! itself to the claim rather than recording itself; the stream records it
//! when it ends — completed, failed with the error its last frame carried,
//! or abandoned — and re-enters the span as it does, which is what moves the
//! exported end time (`tracing-opentelemetry` ends a span at its last exit).
//! The two can finish in either order.

use std::borrow::Cow;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use a2a_protocol_types::error::A2aError;

use super::{CANCELLED, RpcSystem, ServerSpan, WireStatus};

/// How a response stream ended.
#[derive(Debug)]
#[non_exhaustive]
pub enum StreamOutcome {
    /// It ran to its end: the task's last event was sent.
    Completed,
    /// It ended with this error, sent to the peer as its last frame or
    /// status.
    Failed(A2aError),
    /// It stopped before its end: the peer went away, or the server dropped
    /// it. Recorded as `cancelled`, like a call dropped in flight.
    Abandoned,
}

impl StreamOutcome {
    /// The wire status this outcome records, `None` for success.
    fn status(&self, system: RpcSystem) -> Option<Cow<'static, str>> {
        match self {
            Self::Completed => None,
            Self::Failed(e) => Some(e.wire_status(system)),
            Self::Abandoned => Some(Cow::Borrowed(CANCELLED)),
        }
    }
}

tokio::task_local! {
    /// The call whose future, or whose response-building step, is running;
    /// `None` for one that cannot answer with a stream.
    static SLOT: Option<Arc<Slot>>;
}

/// Where a call and the stream it opened meet.
#[derive(Debug)]
pub(super) struct Slot(Mutex<State>);

#[derive(Debug)]
enum State {
    /// No stream has claimed the call.
    Open,
    /// A stream claimed the call, which is still running.
    Claimed,
    /// The stream ended before the call finished.
    Ended(StreamOutcome),
    /// The call finished and waits for its stream.
    Handed(ServerSpan, Instant),
    /// Recorded.
    Done,
}

impl Slot {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(State::Open)))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The call finished with `status` after `started`: recorded now, or
    /// when the stream it opened ends.
    pub(super) fn settle(&self, call: ServerSpan, started: Instant, status: Option<&str>) {
        let mut state = self.state();
        match std::mem::replace(&mut *state, State::Done) {
            // Only a call that succeeded is a stream; a refused one ends here.
            State::Claimed if status.is_none() => *state = State::Handed(call, started),
            State::Ended(outcome) => {
                drop(state);
                let status = outcome.status(call.system);
                call.enter(|| call.finish(started.elapsed(), status.as_deref()));
            }
            _ => {
                drop(state);
                call.finish(started.elapsed(), status);
            }
        }
    }
}

/// Runs `f` as the call `slot` belongs to, so a stream it sets up can claim
/// the call.
pub(super) fn scope<R>(slot: Option<&Arc<Slot>>, f: impl FnOnce() -> R) -> R {
    SLOT.sync_scope(slot.cloned(), f)
}

/// A future run as a call's.
pub(super) type Scoped<F> = tokio::task::futures::TaskLocalFuture<Option<Arc<Slot>>, F>;

/// `fut`, run as the call `slot` belongs to.
pub(super) fn scope_future<F: Future>(slot: Option<&Arc<Slot>>, fut: F) -> Scoped<F> {
    SLOT.scope(slot.cloned(), fut)
}

/// The claim a response stream holds on the call it answers. Dropped
/// without [`end`](Self::end), the stream was abandoned.
#[derive(Debug)]
pub struct StreamEnd(Option<Arc<Slot>>);

impl StreamEnd {
    /// Claims the call running now, if it can answer with a stream and no
    /// stream has claimed it yet. Call it while setting the stream up —
    /// synchronously, inside the call — not from a task spawned for it.
    pub fn claim() -> Option<Self> {
        let slot = SLOT.try_with(Option::clone).ok().flatten()?;
        let mut state = slot.state();
        if !matches!(*state, State::Open) {
            return None;
        }
        *state = State::Claimed;
        drop(state);
        Some(Self(Some(slot)))
    }

    /// Records how the stream ended, and with it the call.
    pub fn end(mut self, outcome: StreamOutcome) {
        if let Some(slot) = self.0.take() {
            finish(&slot, outcome);
        }
    }
}

impl Drop for StreamEnd {
    fn drop(&mut self) {
        if let Some(slot) = self.0.take() {
            finish(&slot, StreamOutcome::Abandoned);
        }
    }
}

fn finish(slot: &Slot, outcome: StreamOutcome) {
    let mut state = slot.state();
    match std::mem::replace(&mut *state, State::Done) {
        State::Handed(call, started) => {
            drop(state);
            let status = outcome.status(call.system);
            // Entered, so the span's exported end is now.
            call.enter(|| call.finish(started.elapsed(), status.as_deref()));
        }
        State::Claimed => *state = State::Ended(outcome),
        other => *state = other,
    }
}

/// Runs `stream` — a stream's forwarding loop — and records the call it
/// answers when it ends, with the outcome it returns. Claims the call when
/// called, so call it inside the call, not inside the task spawned for it.
pub fn spanning_stream<F>(stream: F) -> impl Future<Output = ()>
where
    F: Future<Output = StreamOutcome>,
{
    let end = StreamEnd::claim();
    async move {
        let outcome = stream.await;
        if let Some(end) = end {
            end.end(outcome);
        }
    }
}
