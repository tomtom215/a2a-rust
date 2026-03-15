// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Edge case tests for the SSE parser.

use a2a_client::streaming::SseParser;

fn parse_all(input: &[u8]) -> Vec<a2a_client::streaming::SseFrame> {
    let mut p = SseParser::new();
    p.feed(input);
    let mut frames = Vec::new();
    while let Some(f) = p.next_frame() {
        frames.push(f);
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
