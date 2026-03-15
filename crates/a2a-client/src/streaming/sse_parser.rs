// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Server-Sent Events (SSE) frame parser.
//!
//! Implements a state machine that processes raw bytes and emits [`SseFrame`]s.
//! The parser handles:
//!
//! - Fragmented TCP delivery (bytes arrive in arbitrary-sized chunks).
//! - `data:` lines concatenated with `\n` when a single event spans multiple
//!   `data:` lines.
//! - Keep-alive comment lines (`: keep-alive`), silently ignored.
//! - `event:`, `id:`, and `retry:` fields.
//! - Double-newline event dispatch (`\n\n` terminates each frame).

// ── SseFrame ──────────────────────────────────────────────────────────────────

/// A fully-accumulated SSE event frame.
///
/// A frame is emitted each time the parser sees a blank line (`\n\n`)
/// following at least one `data:` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    /// The event data string (multi-line `data:` values joined by `\n`).
    pub data: String,

    /// Optional event type from `event:` field.
    pub event_type: Option<String>,

    /// Optional event ID from `id:` field.
    pub id: Option<String>,

    /// Optional reconnection timeout hint (milliseconds) from `retry:` field.
    pub retry: Option<u64>,
}

// ── SseParser ─────────────────────────────────────────────────────────────────

/// Stateful SSE byte-stream parser.
///
/// Feed bytes with [`SseParser::feed`] and poll complete frames with
/// [`SseParser::next_frame`].
///
/// The parser buffers bytes internally until a complete line is available,
/// then processes each line according to the SSE spec.
#[derive(Debug, Default)]
pub struct SseParser {
    /// Bytes accumulated since the last newline.
    line_buf: Vec<u8>,
    /// Data lines accumulated since the last blank line.
    data_lines: Vec<String>,
    /// Current `event:` field value.
    event_type: Option<String>,
    /// Current `id:` field value.
    id: Option<String>,
    /// Current `retry:` field value.
    retry: Option<u64>,
    /// Complete frames ready for consumption.
    ready: Vec<SseFrame>,
    /// Whether the UTF-8 BOM has already been checked/stripped.
    bom_checked: bool,
}

impl SseParser {
    /// Creates a new, empty [`SseParser`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of complete frames waiting to be consumed.
    #[must_use]
    pub const fn pending_count(&self) -> usize {
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
                self.line_buf.push(byte);
            }
        }
    }

    /// Returns the next complete [`SseFrame`], or `None` if none are ready.
    pub fn next_frame(&mut self) -> Option<SseFrame> {
        if self.ready.is_empty() {
            None
        } else {
            Some(self.ready.remove(0))
        }
    }

    // ── internals ─────────────────────────────────────────────────────────────

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
            Err(_) => return, // skip malformed UTF-8 lines
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
        self.ready.push(frame);
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
            frames.push(f);
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
        let frame = p.next_frame().expect("expected frame");
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
}
