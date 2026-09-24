// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for what the client parses from a remote agent besides the
//! SSE framing (`sse_parser`) and the protocol types (`json_deser`): an
//! error met mid-stream, an AIP-193 error body, a `Retry-After` header, and
//! the id a WebSocket frame is routed by. None may panic; a `Retry-After`
//! may never ask for more than the one-hour ceiling the client documents.
//!
//! Run with: `cargo +nightly fuzz run client_peer_input`

#![no_main]

use a2a_protocol_client::fuzzing::{
    aip193_error, retry_after, stream_error_frame, websocket_frame_id,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = aip193_error(data);
    if let Some(delay) = retry_after(data) {
        assert!(delay <= std::time::Duration::from_secs(3600), "{delay:?}");
    }
    let Ok(text) = std::str::from_utf8(data) else { return };
    let _ = stream_error_frame(text);
    let _ = websocket_frame_id(text);
});
