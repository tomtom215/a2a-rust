// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Work a call spawns, in the call's trace. Moved out of `mod.rs`, which
//! was at the 500-line limit.

use std::future::Future;

#[cfg(feature = "tracing")]
use super::{UNTRACED, untraced};

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
