// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Audit H13, ADR 0047: the buffered-event byte budget bounds memory, as a
//! counting allocator measures it.
//!
//! A writer pushes 64 events of 256 KiB at a subscriber that never reads,
//! while a processor stores and drops each at once. Without a budget the
//! subscriber's broadcast slots keep all 64, about 16 MiB. With a 2 MiB
//! budget the writer is held at 2 MiB and the rest of its writes fail.
//!
//! One test in its own binary: the allocator counts every thread of the
//! process, so nothing else may run beside it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_server::streaming::{EventQueueManager, EventQueueWriter};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::{ContextId, TaskId};

/// Counts live heap bytes and their high-water mark.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded to `System` unchanged; the counters are
// only arithmetic on the sizes it was given.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract for `alloc` is passed on to `System`.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract for `dealloc` is passed on to `System`.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const EVENT: usize = 256 * 1024;
const EVENTS: usize = 64;
const BUDGET: usize = 2 * 1024 * 1024;

fn chunk() -> StreamResponse {
    StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        Artifact::new("a", vec![Part::text("x".repeat(EVENT))]),
    ))
}

/// The most the heap grew while 64 events were written at an unread
/// subscriber, and how many writes succeeded.
async fn grow(manager: EventQueueManager) -> (usize, usize) {
    let (writer, reader, persisted) = manager
        .get_or_create_with_persistence(&TaskId::new("t"))
        .await;
    let (reader, mut persisted) = (reader.unwrap(), persisted.unwrap());
    // The store's side: take each event and drop it.
    let processor = tokio::spawn(async move { while persisted.recv().await.is_some() {} });
    tokio::task::yield_now().await;
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let mut written = 0;
    for _ in 0..EVENTS {
        if writer.write(chunk()).await.is_ok() {
            written += 1;
        }
        tokio::task::yield_now().await;
    }
    let grown = PEAK.load(Ordering::Relaxed) - base;
    drop((writer, reader));
    manager.destroy_all().await;
    processor.abort();
    (grown, written)
}

#[tokio::test(start_paused = true)]
async fn the_byte_budget_bounds_what_an_unread_subscriber_pins() {
    let (unbounded, all) = grow(EventQueueManager::new()).await;
    assert_eq!(all, EVENTS);
    assert!(
        unbounded >= EVENTS * EVENT,
        "without a budget the subscriber pins every event: grew {unbounded} bytes"
    );

    let budgeted = EventQueueManager::new().with_max_buffered_bytes(BUDGET);
    let (bounded, some) = grow(budgeted).await;
    // Seven 256 KiB events fit in 2 MiB with their JSON framing; the eighth
    // waits out the write timeout and fails, as does every one after.
    assert_eq!(some, 7);
    // What is charged, plus the event being built and refused, plus the
    // deep copy this public persistence channel hands the processor (it
    // carries the original's charge, not one of its own) until it drops it:
    // at most three events past the budget, against 64 without one.
    assert!(
        bounded <= BUDGET + 3 * EVENT,
        "with a {BUDGET}-byte budget the heap grew {bounded} bytes"
    );
}
