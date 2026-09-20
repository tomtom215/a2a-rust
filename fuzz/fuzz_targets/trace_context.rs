// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for the W3C Trace Context parser.
//!
//! `TraceContext::parse` runs on the `traceparent` header of every inbound
//! request, and `with_tracestate` on the `tracestate` beside it — both
//! entirely peer-supplied, reached from the public
//! `RequestHandler::on_send_message(params, streaming, headers)`. The release
//! profile sets `panic = "abort"`, so a panic here is not an error path, it
//! is the process. It found one: a `split_at(55)` behind a *byte*-length
//! check panicked on a multi-byte character straddling that index.
//!
//! Beyond not panicking, two properties are checked:
//!
//! * anything accepted re-emits as something the parser accepts again, and to
//!   the same context — a propagator whose output its own input rejects
//!   corrupts a trace tree silently;
//! * no reserved `trace-flags` bit reaches the wire, per W3C Trace Context
//!   §3.2.2.5.2 ("Vendors MUST set those to zero") and §4.3.
//!
//! Run with: `cargo +nightly fuzz run trace_context`

#![no_main]

use a2a_protocol_types::trace_context::TraceContext;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // Split the input so one corpus entry drives both entry points: the
    // first line is the `traceparent`, the rest is the `tracestate`.
    let (traceparent, tracestate) = match s.split_once('\n') {
        Some((head, tail)) => (head, Some(tail)),
        None => (s, None),
    };

    // Errors are expected and fine; panics are bugs.
    let Ok(parsed) = TraceContext::parse(traceparent) else {
        return;
    };

    // Re-emission must be a value this parser accepts, and must mean the same
    // thing. `traceparent()` always writes version 00 (§3.2.4), so this is a
    // fixed point rather than merely a round trip.
    let rendered = parsed.traceparent();
    let reparsed = TraceContext::parse(&rendered).expect("our own output must re-parse");
    assert_eq!(reparsed.trace_id(), parsed.trace_id());
    assert_eq!(reparsed.span_id(), parsed.span_id());
    assert_eq!(
        reparsed.traceparent(),
        rendered,
        "re-emission must be idempotent"
    );

    // §3.2.2.5.2 / §4.3: only the sampled bit may leave this process.
    assert_eq!(
        reparsed.flags() & !0x01,
        0,
        "reserved trace-flags bits reached the wire in {rendered:?}"
    );
    assert_eq!(reparsed.is_sampled(), parsed.is_sampled());

    let Some(tracestate) = tracestate else {
        return;
    };
    let Ok(with_state) = parsed.with_tracestate(tracestate) else {
        return;
    };
    if let Some(state) = with_state.tracestate() {
        // §3.3.1.5: truncation removes whole entries, so whatever survives
        // must still fit the published caps and still be a list of members.
        assert!(
            state.len() <= 4096,
            "over the documented cap: {}",
            state.len()
        );
        assert!(state.split(',').count() <= 32, "over 32 list members");
        assert!(
            !state.split(',').any(str::is_empty),
            "truncation must not leave an empty member behind: {state:?}"
        );
        // Attaching the result again must be a no-op: it already fits.
        let again = with_state
            .clone()
            .with_tracestate(state)
            .expect("an already-truncated list is acceptable");
        assert_eq!(again.tracestate(), Some(state));
    }
});
