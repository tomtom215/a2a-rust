// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! SSE client-side streaming support.
//!
//! [`SseParser`] transforms raw bytes into [`SseFrame`]s.
//! [`EventStream`] wraps the parser and deserializes each frame as a
//! [`a2a_types::StreamResponse`].

pub mod event_stream;
pub mod sse_parser;

pub use event_stream::EventStream;
pub use sse_parser::{SseFrame, SseParseError, SseParser};
