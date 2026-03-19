// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! SSE parser state machine implementation.

use std::collections::VecDeque;

use super::types::{SseFrame, SseParseError, DEFAULT_MAX_EVENT_SIZE};

// ── SseParser ─────────────────────────────────────────────────────────────────

/// Stateful SSE byte-stream parser.
///
/// Feed bytes with [`SseParser::feed`] and poll complete frames with
/// [`SseParser::next_frame`].
///
/// The parser buffers bytes internally until a complete line is available,
/// then processes each line according to the SSE spec.
///
/// # Memory limits
///
/// The parser enforces a configurable maximum event size (default 4 MiB) to
/// prevent unbounded memory growth from malicious or malformed streams. When
/// the limit is exceeded, the current event is discarded and an error is
/// queued. Use [`SseParser::with_max_event_size`] to configure the limit.
///
/// The internal frame queue is also bounded (default 4096 frames) to prevent
/// OOM from streams that produce many oversized-event errors without the
/// consumer draining them.
#[derive(Debug)]
pub struct SseParser {
    /// Bytes accumulated since the last newline.
    line_buf: Vec<u8>,
    /// Data lines accumulated since the last blank line.
    data_lines: Vec<String>,
    /// Approximate accumulated size of the current event in bytes.
    current_event_size: usize,
    /// Maximum allowed event size in bytes.
    max_event_size: usize,
    /// Maximum number of frames (including errors) buffered in `ready`.
    max_queued_frames: usize,
    /// Current `event:` field value.
    event_type: Option<String>,
    /// Current `id:` field value.
    id: Option<String>,
    /// Current `retry:` field value.
    retry: Option<u64>,
    /// Complete frames ready for consumption (`VecDeque` for O(1) `pop_front`).
    ready: VecDeque<Result<SseFrame, SseParseError>>,
    /// Whether the UTF-8 BOM has already been checked/stripped.
    bom_checked: bool,
}

/// Default maximum number of frames buffered before the oldest is dropped.
const DEFAULT_MAX_QUEUED_FRAMES: usize = 4096;

impl Default for SseParser {
    fn default() -> Self {
        Self {
            line_buf: Vec::new(),
            data_lines: Vec::new(),
            current_event_size: 0,
            max_event_size: DEFAULT_MAX_EVENT_SIZE,
            max_queued_frames: DEFAULT_MAX_QUEUED_FRAMES,
            event_type: None,
            id: None,
            retry: None,
            ready: VecDeque::new(),
            bom_checked: false,
        }
    }
}

impl SseParser {
    /// Creates a new, empty [`SseParser`] with default limits (4 MiB max event size).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new [`SseParser`] with a custom maximum event size.
    ///
    /// Events exceeding this limit will be discarded and an error queued.
    #[must_use]
    pub fn with_max_event_size(max_event_size: usize) -> Self {
        Self {
            max_event_size,
            ..Self::default()
        }
    }

    /// Sets the maximum number of frames that can be buffered before the
    /// oldest frame is dropped. Prevents unbounded memory growth if the
    /// consumer is slower than the producer.
    #[must_use]
    pub const fn with_max_queued_frames(mut self, max: usize) -> Self {
        self.max_queued_frames = max;
        self
    }

    /// Returns the number of complete frames waiting to be consumed.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.ready.len()
    }

    /// Feeds raw bytes from the SSE stream into the parser.
    ///
    /// After calling `feed`, call [`SseParser::next_frame`] repeatedly until
    /// it returns `None` to consume all complete frames.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut input = bytes;
        // Strip UTF-8 BOM (\xEF\xBB\xBF) if it appears at the very start.
        if !self.bom_checked && self.line_buf.is_empty() {
            if input.starts_with(b"\xEF\xBB\xBF") {
                input = &input[3..];
            }
            // Only check once per stream; after the first feed, BOM position
            // has passed regardless.
            if !input.is_empty() || bytes.len() >= 3 {
                self.bom_checked = true;
            }
        }
        for &byte in input {
            if byte == b'\n' {
                self.process_line();
                self.line_buf.clear();
            } else if byte != b'\r' {
                // Ignore bare \r (Windows-style \r\n handled by ignoring \r).
                // Guard against unbounded line_buf growth from lines without
                // newlines (e.g., a malicious server sending a single very long
                // line). We use 2x max_event_size as the limit since a single
                // line can never legitimately exceed the event size.
                if self.line_buf.len() < self.max_event_size.saturating_mul(2) {
                    self.line_buf.push(byte);
                }
                // Bytes beyond the limit are silently dropped; the event will
                // eventually be rejected by the max_event_size check when the
                // line is processed.
            }
        }
    }

    /// Returns the next complete [`SseFrame`], or `None` if none are ready.
    ///
    /// Returns `Err` if an event exceeded the maximum size limit.
    pub fn next_frame(&mut self) -> Option<Result<SseFrame, SseParseError>> {
        self.ready.pop_front()
    }

    // ── internals ─────────────────────────────────────────────────────────────

    /// Pushes a frame result onto the ready queue, dropping the oldest if
    /// the queue exceeds the configured maximum.
    fn enqueue(&mut self, item: Result<SseFrame, SseParseError>) {
        if self.ready.len() >= self.max_queued_frames {
            self.ready.pop_front();
        }
        self.ready.push_back(item);
    }

    fn process_line(&mut self) {
        // Strip BOM if present at start of first line (handles fragmented BOM).
        if !self.bom_checked {
            if self.line_buf.starts_with(b"\xEF\xBB\xBF") {
                self.line_buf.drain(..3);
            }
            self.bom_checked = true;
        }
        let line = match std::str::from_utf8(&self.line_buf) {
            Ok(s) => s.to_owned(),
            Err(_) => {
                // Use lossy conversion instead of silently dropping the line.
                // This preserves valid portions while replacing invalid bytes
                // with U+FFFD, preventing data loss on fragmented multi-byte
                // sequences delivered across TCP chunk boundaries.
                String::from_utf8_lossy(&self.line_buf).into_owned()
            }
        };

        if line.is_empty() {
            // Blank line → dispatch frame if we have data.
            self.dispatch_frame();
            return;
        }

        if line.starts_with(':') {
            // Comment line (e.g. `: keep-alive`) — silently ignore.
            return;
        }

        // Split on the first `:` to get field name and value.
        let (field, value) = line.find(':').map_or_else(
            || (line.as_str(), String::new()),
            |pos| {
                let field = &line[..pos];
                let value = line[pos + 1..].trim_start_matches(' ');
                (field, value.to_owned())
            },
        );

        // Track event size for memory protection.
        self.current_event_size += value.len();
        if self.current_event_size > self.max_event_size {
            // Discard the current event and queue an error.
            let error = SseParseError::EventTooLarge {
                limit: self.max_event_size,
                actual: self.current_event_size,
            };
            self.data_lines.clear();
            self.event_type = None;
            self.current_event_size = 0;
            self.enqueue(Err(error));
            return;
        }

        match field {
            "data" => self.data_lines.push(value),
            "event" => self.event_type = Some(value),
            "id" => {
                if value.contains('\0') {
                    // Spec: id with null byte clears the last event ID.
                    self.id = None;
                } else {
                    self.id = Some(value);
                }
            }
            "retry" => {
                if let Ok(ms) = value.parse::<u64>() {
                    self.retry = Some(ms);
                }
            }
            _ => {
                // Unknown field — ignore per spec.
            }
        }
    }

    fn dispatch_frame(&mut self) {
        if self.data_lines.is_empty() {
            // No data lines → not a real event; reset event-type only.
            self.event_type = None;
            self.current_event_size = 0;
            return;
        }

        // Join data lines with `\n`; remove trailing `\n` if present.
        let mut data = self.data_lines.join("\n");
        if data.ends_with('\n') {
            data.pop();
        }

        let frame = SseFrame {
            data,
            event_type: self.event_type.take(),
            id: self.id.clone(), // id persists across events per spec
            retry: self.retry,
        };

        self.data_lines.clear();
        self.current_event_size = 0;
        self.enqueue(Ok(frame));
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(input: &str) -> Vec<SseFrame> {
        let mut p = SseParser::new();
        p.feed(input.as_bytes());
        let mut frames = Vec::new();
        while let Some(f) = p.next_frame() {
            frames.push(f.expect("unexpected error"));
        }
        frames
    }

    #[test]
    fn parse_single_data_event() {
        let frames = parse_all("data: hello world\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "hello world");
    }

    #[test]
    fn parse_multiline_data() {
        let frames = parse_all("data: line1\ndata: line2\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "line1\nline2");
    }

    #[test]
    fn parse_two_events() {
        let frames = parse_all("data: first\n\ndata: second\n\n");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].data, "first");
        assert_eq!(frames[1].data, "second");
    }

    #[test]
    fn ignore_keepalive_comment() {
        let frames = parse_all(": keep-alive\n\ndata: real\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "real");
    }

    #[test]
    fn parse_event_type() {
        let frames = parse_all("event: status-update\ndata: {}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event_type.as_deref(), Some("status-update"));
    }

    #[test]
    fn parse_id_field() {
        let frames = parse_all("id: 42\ndata: hello\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].id.as_deref(), Some("42"));
    }

    #[test]
    fn parse_retry_field() {
        let frames = parse_all("retry: 5000\ndata: hello\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].retry, Some(5000));
    }

    #[test]
    fn fragmented_delivery() {
        let mut p = SseParser::new();
        // Feed bytes one at a time to simulate fragmented TCP.
        for byte in b"data: fragmented\n\n" {
            p.feed(std::slice::from_ref(byte));
        }
        let frame = p.next_frame().expect("expected frame").expect("no error");
        assert_eq!(frame.data, "fragmented");
    }

    #[test]
    fn blank_line_without_data_is_ignored() {
        let frames = parse_all("event: ping\n\ndata: real\n\n");
        // First blank line (no data) should produce no frame.
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "real");
    }

    #[test]
    fn json_data_roundtrip() {
        let json = r#"{"jsonrpc":"2.0","id":"1","result":{"kind":"task"}}"#;
        let input = format!("data: {json}\n\n");
        let frames = parse_all(&input);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, json);
    }

    #[test]
    fn event_too_large_returns_error() {
        let mut p = SseParser::with_max_event_size(32);
        // Feed data that exceeds the 32-byte limit.
        let big_line = format!("data: {}\n\n", "x".repeat(64));
        p.feed(big_line.as_bytes());
        let result = p.next_frame().expect("expected result");
        assert!(result.is_err());
        match result.unwrap_err() {
            SseParseError::EventTooLarge { limit, .. } => {
                assert_eq!(limit, 32);
            }
        }
    }

    #[test]
    fn events_after_oversized_event_still_parse() {
        let mut p = SseParser::with_max_event_size(16);
        // First event is too large.
        let big = format!("data: {}\n\n", "x".repeat(32));
        // Second event is small enough.
        let small = "data: ok\n\n";
        p.feed(big.as_bytes());
        p.feed(small.as_bytes());

        let first = p.next_frame().expect("expected result");
        assert!(first.is_err());

        let second = p.next_frame().expect("expected result");
        assert_eq!(second.unwrap().data, "ok");
    }

    /// Bug #33: `next_frame` used `Vec::remove(0)` which is O(n).
    /// Verify `VecDeque`-based dequeue works correctly for many events.
    #[test]
    fn many_events_dequeue_correctly() {
        let mut input = String::new();
        for i in 0..100 {
            use std::fmt::Write;
            let _ = write!(input, "data: event-{i}\n\n");
        }
        let mut p = SseParser::new();
        p.feed(input.as_bytes());
        assert_eq!(p.pending_count(), 100);

        for i in 0..100 {
            let frame = p.next_frame().unwrap().unwrap();
            assert_eq!(frame.data, format!("event-{i}"));
        }
        assert!(p.next_frame().is_none());
    }

    /// Bug #34: Malformed UTF-8 lines were silently dropped.
    /// Now uses lossy conversion to preserve data.
    #[test]
    fn malformed_utf8_uses_lossy_conversion() {
        let mut p = SseParser::new();
        // Feed "data: " + invalid byte + valid suffix, then double-newline.
        let mut bytes = b"data: hello\xFFworld\n\n".to_vec();
        p.feed(&bytes);
        let frame = p.next_frame().unwrap().unwrap();
        // The invalid byte should be replaced with U+FFFD.
        assert!(frame.data.contains("hello"));
        assert!(frame.data.contains("world"));
        assert!(frame.data.contains('\u{FFFD}'));

        // Also test that a fully valid line after the malformed one still works.
        bytes = b"data: clean\n\n".to_vec();
        p.feed(&bytes);
        let frame2 = p.next_frame().unwrap().unwrap();
        assert_eq!(frame2.data, "clean");
    }

    #[test]
    fn display_event_too_large_error() {
        let err = SseParseError::EventTooLarge {
            limit: 100,
            actual: 200,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("200") && msg.contains("100"),
            "Display should contain actual and limit values, got: {msg}"
        );
        assert!(
            msg.contains("too large"),
            "Display should describe the error, got: {msg}"
        );
    }

    #[test]
    fn default_max_event_size_is_16mib() {
        // DEFAULT_MAX_EVENT_SIZE = 16 * 1024 * 1024 = 16_777_216
        // Mutation `replace * with +` at position 42 yields 16 * 1024 + 1024 = 17_408.
        // Feed data larger than 17_408 to kill that mutation.
        let data = format!("data: {}\n\n", "x".repeat(20_000));
        let mut parser = SseParser::new();
        parser.feed(data.as_bytes());
        let frame = parser.next_frame().expect("should have a frame");
        assert!(
            frame.is_ok(),
            "20_000-byte event should be within default 16 MiB limit"
        );
    }

    #[test]
    fn default_max_event_size_accepts_over_one_mib() {
        // Kills mutation: first `*` → `+` in `16 * 1024 * 1024`
        // which gives 16 + 1024 * 1024 = 1_048_592 (~1 MiB).
        // A 1.1 MiB event should pass the real 16 MiB limit but fail the mutated ~1 MiB limit.
        let data = format!("data: {}\n\n", "x".repeat(1_100_000));
        let mut parser = SseParser::new();
        parser.feed(data.as_bytes());
        let frame = parser.next_frame().expect("should have a frame");
        assert!(
            frame.is_ok(),
            "1.1 MiB event should be within default 16 MiB limit"
        );
    }

    #[test]
    fn bom_at_stream_start_is_stripped() {
        // Tests BOM stripping in feed() — covers mutations on lines 157 and 163.
        let mut p = SseParser::new();
        // Feed BOM followed by a data event.
        let mut input = Vec::new();
        input.extend_from_slice(b"\xEF\xBB\xBF");
        input.extend_from_slice(b"data: after-bom\n\n");
        p.feed(&input);
        let frame = p.next_frame().unwrap().unwrap();
        assert_eq!(frame.data, "after-bom");
    }

    #[test]
    fn bom_only_stripped_at_start_not_later() {
        // After BOM is checked, later BOM-like bytes in line_buf should NOT be stripped
        // by process_line. This kills mutation: `delete ! in process_line` (line 189).
        // If mutated to `self.bom_checked`, process_line would incorrectly strip BOM
        // bytes from later lines when bom_checked=true.
        let mut p = SseParser::new();
        // First feed: normal data, sets bom_checked = true.
        p.feed(b"data: first\n\n");
        let _ = p.next_frame().unwrap().unwrap();
        // Second feed: line_buf will start with BOM bytes (\xEF\xBB\xBF).
        // These bytes represent a line that starts with BOM followed by "data: second".
        // Since bom_checked=true, process_line should NOT strip them.
        // The line will be: "\xEF\xBB\xBFdata: second" which is an unknown field
        // (the BOM chars prefix "data"), so no frame is produced from that line.
        // Then we send a normal event to verify the parser still works.
        p.feed(b"\xEF\xBB\xBFdata: second\n\ndata: third\n\n");
        // If the mutation were applied (delete !), process_line would strip BOM
        // from lines where bom_checked=true, turning "\xEF\xBB\xBFdata: second"
        // into "data: second", producing a frame with data="second".
        // Without the mutation, BOM is NOT stripped, so the first line is unknown
        // and only "third" produces a frame.
        let frame = p.next_frame().unwrap().unwrap();
        assert_eq!(
            frame.data, "third",
            "BOM should not be stripped from later lines; 'second' line should be ignored"
        );
        // There should be no more frames (the BOM-prefixed line was not parsed as data).
        assert!(p.next_frame().is_none());
    }

    #[test]
    fn bom_fragmented_across_feeds() {
        // Feed BOM as a complete 3-byte sequence at the start, followed by data.
        // This tests the BOM stripping in feed() when line_buf is empty.
        let mut p = SseParser::new();
        p.feed(b"\xEF\xBB\xBFdata: after-bom\n\n");
        let frame = p.next_frame().unwrap().unwrap();
        assert_eq!(frame.data, "after-bom");
    }

    #[test]
    fn empty_feed_before_bom_does_not_mark_checked() {
        // Feeding empty bytes should not set bom_checked = true.
        // This covers: `!input.is_empty() || bytes.len() >= 3` mutations.
        let mut p = SseParser::new();
        p.feed(b""); // empty feed
                     // Now feed BOM + data — BOM should still be stripped.
        let mut input = Vec::new();
        input.extend_from_slice(b"\xEF\xBB\xBF");
        input.extend_from_slice(b"data: still-works\n\n");
        p.feed(&input);
        let frame = p.next_frame().unwrap().unwrap();
        assert_eq!(frame.data, "still-works");
    }

    #[test]
    fn event_exactly_at_max_size_is_accepted() {
        // Tests `>` vs `>=` mutation on line 229.
        // current_event_size > max_event_size means exactly equal should be accepted.
        let limit = 10;
        let mut p = SseParser::with_max_event_size(limit);
        // "data: " is the field prefix, value is exactly 10 bytes.
        let data = format!("data: {}\n\n", "x".repeat(limit));
        p.feed(data.as_bytes());
        let result = p.next_frame().expect("should have a frame");
        assert!(
            result.is_ok(),
            "Event exactly at max_event_size should be accepted, not rejected"
        );
        assert_eq!(result.unwrap().data, "x".repeat(limit));
    }

    #[test]
    fn event_one_byte_over_max_size_is_rejected() {
        // Complement to the above: one byte over should be rejected.
        let limit = 10;
        let mut p = SseParser::with_max_event_size(limit);
        let data = format!("data: {}\n\n", "x".repeat(limit + 1));
        p.feed(data.as_bytes());
        let result = p.next_frame().expect("should have a frame");
        assert!(
            result.is_err(),
            "Event one byte over limit should be rejected"
        );
    }

    #[test]
    fn bom_at_line_start_not_stripped_after_first_event() {
        // Kill mutation: `delete ! in process_line` (line 189).
        // If `!self.bom_checked` becomes `self.bom_checked`, BOM bytes at line_buf
        // start would be stripped on all lines AFTER the first, corrupting data.
        let mut p = SseParser::new();
        // Normal first event sets bom_checked = true.
        p.feed(b"data: first\n\n");
        let f1 = p.next_frame().unwrap().unwrap();
        assert_eq!(f1.data, "first");

        // Now send a line whose line_buf starts with BOM bytes.
        // This is an "unknown field" line (field name starts with BOM chars).
        // After it, send a normal data line and dispatch.
        // If mutation applied, BOM would be stripped making the field name "data"
        // and we'd get frame data = "corrupted".
        p.feed(b"\xEF\xBB\xBFdata: corrupted\ndata: clean\n\n");
        let f2 = p.next_frame().unwrap().unwrap();
        // Only "clean" should be in the frame; the BOM-prefixed line is an unknown field.
        assert_eq!(f2.data, "clean");
    }

    #[test]
    fn bom_not_stripped_on_second_feed_kills_and_or_mutation() {
        // Kill mutation: `replace && with || in SseParser::feed` (line 157)
        // With &&→||, the feed BOM check runs when EITHER bom_checked=false
        // OR line_buf is empty. After first event, bom_checked=true but line_buf
        // is empty → with mutation the check runs and strips BOM incorrectly.
        let mut p = SseParser::new();
        p.feed(b"data: first\n\n");
        let _ = p.next_frame().unwrap().unwrap();
        // Second feed starts with raw BOM bytes.
        // With correct code (&&): bom_checked=true → check doesn't run → BOM NOT stripped.
        // With mutation (||): line_buf empty → check runs → BOM stripped → "data: second" parsed.
        p.feed(b"\xEF\xBB\xBFdata: second\n\n");
        // BOM should NOT be stripped, so field name is "\u{FEFF}data" (unknown) → no frame.
        assert!(
            p.next_frame().is_none(),
            "BOM at start of second feed should NOT be stripped (bom_checked=true)"
        );
    }

    #[test]
    fn bom_only_three_bytes_marks_checked() {
        // Kill mutation: `replace >= with < in SseParser::feed` (line 163)
        // Feed exactly 3 BOM bytes. After stripping, input is empty.
        // `!input.is_empty() || bytes.len() >= 3` → `false || true` → true → bom_checked = true.
        // With >= → <: `false || (3 < 3)` → `false || false` → false → bom_checked stays false.
        let mut p = SseParser::new();
        p.feed(b"\xEF\xBB\xBF"); // exactly 3 BOM bytes
                                 // If bom_checked stayed false (mutation), next feed would try to strip BOM again.
                                 // Feed normal data — should work regardless.
        p.feed(b"data: ok\n\n");
        let frame = p.next_frame().unwrap().unwrap();
        assert_eq!(frame.data, "ok");
        // Now feed BOM+data again. With correct code: bom_checked=true, BOM not stripped.
        // With mutation: bom_checked=false, BOM stripped, "data: again" parsed → frame.
        p.feed(b"\xEF\xBB\xBFdata: again\n\n");
        assert!(
            p.next_frame().is_none(),
            "After first BOM-only feed (3 bytes), bom_checked should be true"
        );
    }

    #[test]
    fn bom_only_feed_then_bom_data_kills_or_to_and_mutation() {
        // Kill mutation: `replace || with && in SseParser::feed` (line 163)
        // Feed exactly 3 BOM bytes. After stripping, input is empty.
        // Original: `!input.is_empty() || bytes.len() >= 3` → `false || true` → true
        // Mutated:  `!input.is_empty() && bytes.len() >= 3` → `false && true` → false
        // With mutation, bom_checked stays false, so a second BOM would be stripped.
        let mut p = SseParser::new();
        p.feed(b"\xEF\xBB\xBF"); // exactly 3 BOM bytes
                                 // Immediately feed BOM + data. If bom_checked was not set (mutation),
                                 // the BOM is stripped again and "data: stolen" is parsed as a frame.
        p.feed(b"\xEF\xBB\xBFdata: stolen\n\n");
        // With correct code: bom_checked=true after first feed → BOM not stripped
        // → line is unknown field → no frame.
        assert!(
            p.next_frame().is_none(),
            "BOM-only feed should mark bom_checked; second BOM must not be stripped"
        );
    }

    /// Multiple data lines are joined with newlines.
    #[test]
    fn multiple_data_lines_joined() {
        let input = "data: hello\ndata: world\n\n";
        let mut p = SseParser::new();
        p.feed(input.as_bytes());
        let frame = p.next_frame().unwrap().unwrap();
        assert_eq!(frame.data, "hello\nworld");
    }

    /// BOM at the very start of a stream is stripped.
    #[test]
    fn bom_at_stream_start_stripped() {
        let mut p = SseParser::new();
        p.feed(b"\xEF\xBB\xBFdata: bom-test\n\n");
        let frame = p.next_frame().unwrap().unwrap();
        assert_eq!(frame.data, "bom-test");
    }

    #[test]
    fn short_non_bom_feed_then_bom_feed() {
        // Feed a short (< 3 bytes) non-empty, non-BOM input first.
        // This should set bom_checked = false still (input not empty, bytes.len() < 3
        // but input is not empty so the condition is true — bom_checked becomes true).
        // Then feeding BOM should NOT strip it.
        let mut p = SseParser::new();
        p.feed(b"d"); // single non-BOM byte, not empty so bom_checked = true
        p.feed(b"ata: hello\n\n");
        let frame = p.next_frame().unwrap().unwrap();
        assert_eq!(frame.data, "hello");
    }

    #[test]
    fn queue_bound_drops_oldest_when_full() {
        let mut p = SseParser::new().with_max_queued_frames(3);
        // Feed 5 events without consuming any.
        for i in 0..5 {
            let data = format!("data: event-{i}\n\n");
            p.feed(data.as_bytes());
        }
        // Queue should be capped at 3 — the 2 oldest were dropped.
        assert_eq!(p.pending_count(), 3);
        let frame = p.next_frame().unwrap().unwrap();
        assert_eq!(
            frame.data, "event-2",
            "oldest frames should have been dropped"
        );
    }

    #[test]
    fn queue_bound_drops_oldest_errors_too() {
        let mut p = SseParser::with_max_event_size(5).with_max_queued_frames(2);
        // Feed 3 oversized events to produce 3 errors.
        for _ in 0..3 {
            let data = format!("data: {}\n\n", "x".repeat(20));
            p.feed(data.as_bytes());
        }
        assert_eq!(p.pending_count(), 2, "queue should be bounded at 2");
    }
}
