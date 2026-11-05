// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The byte budget on the queue path (ADR 0047): what holds a charge, what
//! returns it, a write that waits and one that fails without a hole, and a
//! lease refused while the budget is nearly spent.

use std::time::Duration;

use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::{ContextId, TaskId};

use super::{EventQueueManager, EventQueueReader, EventQueueWriter, QueueConsumer, QueueLease};

/// An artifact event whose serialized size is a little over `text` bytes.
fn chunk(text: usize) -> StreamResponse {
    StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        Artifact::new("a", vec![Part::text("x".repeat(text))]),
    ))
}

/// The serialized size the writer charges for `chunk(text)`.
fn size(text: usize) -> usize {
    serde_json::to_vec(&chunk(text)).unwrap().len()
}

async fn lease(
    manager: &EventQueueManager,
    task: &str,
) -> (
    std::sync::Arc<super::InMemoryQueueWriter>,
    super::InMemoryQueueReader,
    super::PersistenceRx,
) {
    match manager
        .lease(&TaskId::new(task), QueueConsumer::Background, None)
        .await
    {
        QueueLease::Created {
            writer,
            reader,
            persistence_rx,
        } => (writer, reader, persistence_rx.unwrap()),
        _ => panic!("expected a new queue"),
    }
}

/// Two events fit; the third waits for the write timeout and fails, taking
/// no position; once the subscriber is gone and the processor has the
/// events, the bytes are back and the next write takes position 3.
#[tokio::test(start_paused = true)]
async fn a_write_past_the_budget_waits_then_fails_without_a_hole() {
    let one = size(10_000);
    let manager = EventQueueManager::new()
        .with_write_timeout(Duration::from_secs(2))
        .with_max_buffered_bytes(2 * one + one / 2);
    let (writer, reader, mut persisted) = lease(&manager, "t").await;
    writer.write(chunk(10_000)).await.unwrap();
    writer.write(chunk(10_000)).await.unwrap();
    assert_eq!(manager.buffered_bytes(), Some((2 * one, 2 * one + one / 2)));

    let started = tokio::time::Instant::now();
    let refused = writer.write(chunk(10_000)).await.unwrap_err();
    assert_eq!(started.elapsed(), Duration::from_secs(2));
    assert!(refused.message.contains("budget"), "{}", refused.message);

    // The processor stores and drops both; the unread subscriber still
    // pins them.
    for expected in [1, 2] {
        let event = persisted.recv().await.unwrap();
        assert_eq!(event.as_ref().as_ref().unwrap().seq, Some(expected));
    }
    assert_eq!(manager.buffered_bytes().unwrap().0, 2 * one);
    drop(reader);
    assert_eq!(manager.buffered_bytes().unwrap().0, 0, "nothing holds them");

    writer.write(chunk(10_000)).await.unwrap();
    let next = persisted.recv().await.unwrap();
    assert_eq!(next.as_ref().as_ref().unwrap().seq, Some(3), "no hole");
}

/// A write that finds the budget spent is admitted the moment a holder
/// returns enough bytes, not at its deadline.
#[tokio::test(start_paused = true)]
async fn a_waiting_write_goes_as_soon_as_a_subscriber_reads() {
    let one = size(10_000);
    let manager = EventQueueManager::new()
        .with_write_timeout(Duration::from_secs(5))
        .with_max_buffered_bytes(one + one / 2);
    let (writer, mut reader, mut persisted) = lease(&manager, "t").await;
    writer.write(chunk(10_000)).await.unwrap();
    drop(persisted.recv().await.unwrap());

    let started = tokio::time::Instant::now();
    let second = {
        let writer = std::sync::Arc::clone(&writer);
        tokio::spawn(async move { writer.write(chunk(10_000)).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    // The subscriber reads the first event and drops it: its bytes return.
    drop(reader.read().await.unwrap().unwrap());
    second.await.unwrap().unwrap();
    assert_eq!(started.elapsed(), Duration::from_millis(300));
}

/// A new queue is refused while less than a tenth of the budget is free,
/// and leased again once a tenth is.
#[tokio::test]
async fn a_new_queue_needs_a_tenth_of_the_budget_free() {
    let one = size(10_000);
    // The event takes 91 % of the budget.
    let manager = EventQueueManager::new().with_max_buffered_bytes(one * 100 / 91);
    let (writer, reader, mut persisted) = lease(&manager, "t").await;
    writer.write(chunk(10_000)).await.unwrap();
    assert!(matches!(
        manager
            .lease(&TaskId::new("u"), QueueConsumer::Background, None)
            .await,
        QueueLease::BytesExhausted
    ));
    drop(persisted.recv().await.unwrap());
    drop(reader);
    let (_w, _r, _p) = lease(&manager, "u").await;
    assert_eq!(manager.active_count().await, 2);
}

/// Without a budget nothing is charged and nothing is reported.
#[tokio::test]
async fn without_a_budget_nothing_is_charged() {
    let manager = EventQueueManager::new();
    assert_eq!(manager.buffered_bytes(), None);
    let (writer, _reader, mut persisted) = lease(&manager, "t").await;
    writer.write(chunk(10)).await.unwrap();
    let event = persisted.recv().await.unwrap();
    assert!(event.as_ref().as_ref().unwrap().charge.is_none());
    assert!(format!("{manager:?}").contains("byte_budget: None"));
}
