// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for SSE frame formatting and streaming.

use a2a_server::streaming::sse::{write_event, write_keep_alive};

#[test]
fn write_event_basic() {
    let frame = write_event("message", r#"{"hello":"world"}"#);
    let text = std::str::from_utf8(&frame).unwrap();
    assert!(text.starts_with("event: message\n"));
    assert!(text.contains("data: {\"hello\":\"world\"}\n"));
    assert!(text.ends_with("\n\n"));
}

#[test]
fn write_event_multiline_data() {
    let data = "line1\nline2\nline3";
    let frame = write_event("message", data);
    let text = std::str::from_utf8(&frame).unwrap();
    assert!(text.contains("data: line1\n"));
    assert!(text.contains("data: line2\n"));
    assert!(text.contains("data: line3\n"));
}

#[test]
fn write_event_empty_data() {
    let frame = write_event("ping", "");
    let text = std::str::from_utf8(&frame).unwrap();
    assert!(text.starts_with("event: ping\n"));
    // Empty string has no lines, so no data: lines
    assert!(text.ends_with("\n"));
}

#[test]
fn write_keep_alive_format() {
    let frame = write_keep_alive();
    let text = std::str::from_utf8(&frame).unwrap();
    assert_eq!(text, ": keep-alive\n\n");
}

#[test]
fn write_event_special_characters() {
    let frame = write_event("message", "data with: colons and = equals");
    let text = std::str::from_utf8(&frame).unwrap();
    assert!(text.contains("data: data with: colons and = equals\n"));
}
