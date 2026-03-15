// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Streaming infrastructure for SSE responses and event queues.

pub mod event_queue;
pub mod sse;

pub use event_queue::{
    EventQueueManager, EventQueueReader, EventQueueWriter, InMemoryQueueReader,
    InMemoryQueueWriter, DEFAULT_MAX_EVENT_SIZE, DEFAULT_QUEUE_CAPACITY,
};
pub use sse::{build_sse_response, SseBodyWriter};
