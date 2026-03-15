// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Edge case tests for the SSE parser.

use a2a_client::streaming::{SseParseError, SseParser};

fn parse_all(input: &[u8]) -> Vec<a2a_client::streaming::SseFrame> {
    let mut p = SseParser::new();
    p.feed(input);
    let mut frames = Vec::new();
    while let Some(f) = p.next_frame() {
        frames.push(f.expect("unexpected parse error"));
    }
    frames
}

#[test]
fn utf8_bom_at_start_is_handled() {
    // UTF-8 BOM: EF BB BF followed by data
    let mut input = vec![0xEF, 0xBB, 0xBF];
    input.extend_from_slice(b"data: hello\n\n");
    let frames = parse_all(&input);
    // The BOM may appear in the data or be stripped; either way, parser
    // should not crash or produce garbage.
    assert_eq!(frames.len(), 1);
}

#[test]
fn data_with_embedded_json_newlines() {
    // JSON that spans multiple data: lines
    let input = b"data: {\"key\":\n\
                   data: \"value\"}\n\n";
    let frames = parse_all(input);
    assert_eq!(frames.len(), 1);
    // Multi-line data should be joined with \n
    assert!(frames[0].data.contains("key"));
    assert!(frames[0].data.contains("value"));
}

#[test]
fn numeric_id_zero_is_valid() {
    let frames = parse_all(b"id: 0\ndata: test\n\n");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].id.as_deref(), Some("0"));
}

#[test]
fn very_long_data_field() {
    // 100KB of data
    let data = "a".repeat(100_000);
    let input = format!("data: {data}\n\n");
    let frames = parse_all(input.as_bytes());
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data.len(), 100_000);
}

#[test]
fn empty_data_field() {
    let frames = parse_all(b"data:\n\n");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data, "");
}

#[test]
fn data_with_no_space_after_colon() {
    let frames = parse_all(b"data:nospace\n\n");
    assert_eq!(frames.len(), 1);
    // Per SSE spec, first space after colon is optional
    assert!(frames[0].data == "nospace" || frames[0].data == " nospace");
}

#[test]
fn crlf_line_endings() {
    let frames = parse_all(b"data: hello\r\n\r\n");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data, "hello");
}

#[test]
fn id_persists_across_events() {
    let input = b"id: abc\ndata: first\n\ndata: second\n\n";
    let frames = parse_all(input);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].id.as_deref(), Some("abc"));
    // Per SSE spec, the id persists until explicitly changed
    assert_eq!(frames[1].id.as_deref(), Some("abc"));
}

#[test]
fn event_type_does_not_persist() {
    let input = b"event: special\ndata: first\n\ndata: second\n\n";
    let frames = parse_all(input);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].event_type.as_deref(), Some("special"));
    // Per SSE spec, event type resets to None after each dispatch
    assert!(frames[1].event_type.is_none());
}

#[test]
fn unknown_field_is_ignored() {
    let frames = parse_all(b"custom: ignored\ndata: valid\n\n");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data, "valid");
}

#[test]
fn multiple_colons_in_value() {
    let frames = parse_all(b"data: http://example.com:8080/path\n\n");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data, "http://example.com:8080/path");
}

#[test]
fn retry_with_non_numeric_value_is_ignored() {
    let frames = parse_all(b"retry: abc\ndata: test\n\n");
    assert_eq!(frames.len(), 1);
    assert!(frames[0].retry.is_none());
}

#[test]
fn consecutive_blank_lines_produce_no_extra_events() {
    let frames = parse_all(b"data: first\n\n\n\ndata: second\n\n");
    assert_eq!(frames.len(), 2);
}

// ── Field name only (no colon) ───────────────────────────────────────────────

#[test]
fn field_name_only_no_colon() {
    // Per SSE spec, a line with no colon is treated as a field name with an
    // empty string value. Unknown fields are ignored.
    let frames = parse_all(b"data\ndata: real\n\n");
    assert_eq!(frames.len(), 1);
    // "data" with no colon should be treated as data field with empty value.
    // Combined with the second line, we expect two data lines joined by \n.
    assert!(frames[0].data.contains("real"));
}

#[test]
fn field_name_only_data_dispatches() {
    // A bare "data" line (no colon) followed by a blank line should dispatch
    // a frame with empty data.
    let frames = parse_all(b"data\n\n");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data, "");
}

#[test]
fn field_name_only_unknown_is_ignored() {
    // Unknown field names without a colon are silently ignored per spec.
    let frames = parse_all(b"foobar\ndata: valid\n\n");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data, "valid");
}

// ── Extremely long field names ───────────────────────────────────────────────

#[test]
fn extremely_long_field_name_is_ignored() {
    // A line with a very long unknown field name should be silently ignored.
    let long_field = "x".repeat(10_000);
    let input = format!("{long_field}: value\ndata: valid\n\n");
    let frames = parse_all(input.as_bytes());
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data, "valid");
}

#[test]
fn extremely_long_field_name_no_colon() {
    // A line with a very long field name and no colon.
    let long_field = "x".repeat(10_000);
    let input = format!("{long_field}\ndata: valid\n\n");
    let frames = parse_all(input.as_bytes());
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data, "valid");
}

// ── Event size limit ─────────────────────────────────────────────────────────

#[test]
fn with_max_event_size_rejects_oversized_event() {
    let mut p = SseParser::with_max_event_size(64);
    // Feed data that exceeds the 64-byte limit.
    let big = format!("data: {}\n\n", "z".repeat(100));
    p.feed(big.as_bytes());

    let result = p.next_frame().expect("should have a result");
    assert!(result.is_err());
    match result.unwrap_err() {
        SseParseError::EventTooLarge { limit, actual } => {
            assert_eq!(limit, 64);
            assert!(actual > 64);
        }
    }
}

#[test]
fn with_max_event_size_allows_small_events() {
    let mut p = SseParser::with_max_event_size(1024);
    p.feed(b"data: small\n\n");

    let result = p.next_frame().expect("should have a result");
    let frame = result.expect("should not error");
    assert_eq!(frame.data, "small");
}

#[test]
fn with_max_event_size_recovery_after_oversized() {
    // After rejecting an oversized event, the parser should still parse
    // subsequent smaller events correctly.
    let mut p = SseParser::with_max_event_size(32);

    let big = format!("data: {}\n\n", "a".repeat(64));
    let small = "data: ok\n\n";
    p.feed(big.as_bytes());
    p.feed(small.as_bytes());

    let first = p.next_frame().expect("first result");
    assert!(first.is_err(), "oversized event should error");

    let second = p.next_frame().expect("second result");
    assert_eq!(second.unwrap().data, "ok");
}
