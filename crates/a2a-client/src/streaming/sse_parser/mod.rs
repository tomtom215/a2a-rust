// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

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
//! - Configurable maximum event size to prevent unbounded memory growth.

mod parser;
mod types;

pub use parser::SseParser;
pub use types::{SseFrame, SseParseError};
