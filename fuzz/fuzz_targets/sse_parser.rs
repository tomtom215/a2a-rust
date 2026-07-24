// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for the client SSE parser.
//!
//! Feeds arbitrary bytes — split into arbitrary chunks — into `SseParser`
//! and drains frames. Byte-boundary splits are where incremental parsers
//! break, so the input's first byte selects a chunk size. No input may
//! panic, and the bounded-queue invariant must hold (OOM guard).
//!
//! Run with: `cargo +nightly fuzz run sse_parser`

#![no_main]

use a2a_protocol_client::streaming::SseParser;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    // First byte picks a chunk size in 1..=64 so we exercise arbitrary
    // byte-boundary splits of the same stream.
    let chunk = (data[0] as usize % 64) + 1;
    let body = &data[1..];

    let mut parser = SseParser::with_max_event_size(4096).with_max_queued_frames(16);
    for slice in body.chunks(chunk) {
        parser.feed(slice);
        // Drain available frames; the queue is bounded, so this always
        // terminates.
        while let Some(frame) = parser.next_frame() {
            let _ = frame;
        }
    }
    // Final drain after the stream ends.
    while let Some(frame) = parser.next_frame() {
        let _ = frame;
    }
});
