// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Where a delegation started, carried into the work it spawns.
//!
//! A `tokio::spawn` starts from nothing: no current span, no ambient
//! [`CurrentTrace`](crate::CurrentTrace). A delegation's lease renewals and
//! the cancel it sends when dropped are spawned, so without this each of
//! their calls opened a trace of its own, unconnected to the task that
//! delegated. Both run in the span that was current when the delegation
//! started, and under its ambient trace, so their `CLIENT` spans — and the
//! `traceparent` they send — are in the parent task's trace.
//!
//! Holding the span keeps it open while the delegation lives, which is no
//! longer than the parent task that started it (the renewals stop when the
//! handle drops; the cancel is one call).

use std::future::Future;

use a2a_protocol_types::trace_context::TraceContext;

/// The span and ambient trace a delegation started in.
#[derive(Clone, Debug)]
pub(super) struct Origin {
    #[cfg(feature = "tracing")]
    span: tracing::Span,
    trace: Option<TraceContext>,
}

impl Origin {
    /// Where the caller is now.
    pub(super) fn current() -> Self {
        Self {
            #[cfg(feature = "tracing")]
            span: tracing::Span::current(),
            trace: crate::CurrentTrace::current(),
        }
    }

    /// `fut`, run in this origin's span and ambient trace.
    pub(super) fn carry<F: Future>(&self, fut: F) -> impl Future<Output = F::Output> + use<F> {
        let trace = self.trace.clone();
        #[cfg(feature = "tracing")]
        let span = self.span.clone();
        let fut = async move {
            match trace {
                Some(trace) => crate::CurrentTrace::scope(trace, fut).await,
                None => fut.await,
            }
        };
        #[cfg(feature = "tracing")]
        let fut = tracing::Instrument::instrument(fut, span);
        fut
    }
}
