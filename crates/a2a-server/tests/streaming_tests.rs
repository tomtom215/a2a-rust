// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for the event queue and SSE streaming infrastructure.

use a2a_types::artifact::Artifact;
use a2a_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_types::message::Part;
use a2a_types::task::{TaskId, TaskState, TaskStatus};

use a2a_server::streaming::sse::{write_event, write_keep_alive};
use a2a_server::streaming::{EventQueueManager, EventQueueReader, EventQueueWriter};

// ── Event queue tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn queue_write_and_read() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("task-1");

    let (writer, reader) = manager.get_or_create(&task_id).await;
    let mut reader = reader.expect("should get reader on first call");

    let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: task_id.clone(),
        context_id: "ctx-1".into(),
        status: TaskStatus::new(TaskState::Working),
        metadata: None,
    });

    writer.write(event).await.expect("write");
    drop(writer);

    // Destroy the manager's reference so reader sees channel close.
    manager.destroy(&task_id).await;

    let received = reader.read().await.expect("read");
    let update = received.expect("should be ok");
    assert!(
        matches!(update, StreamResponse::StatusUpdate(u) if u.status.state == TaskState::Working)
    );

    // Channel is closed after writer is dropped.
    assert!(reader.read().await.is_none());
}

#[tokio::test]
async fn queue_multiple_events() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("task-2");

    let (writer, reader) = manager.get_or_create(&task_id).await;
    let mut reader = reader.expect("reader");

    // Write two events.
    writer
        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: task_id.clone(),
            context_id: "ctx".into(),
            status: TaskStatus::new(TaskState::Working),
            metadata: None,
        }))
        .await
        .unwrap();

    writer
        .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            task_id: task_id.clone(),
            context_id: "ctx".into(),
            artifact: Artifact::new("a1", vec![Part::text("data")]),
            append: None,
            last_chunk: Some(true),
            metadata: None,
        }))
        .await
        .unwrap();

    // Drop writer to close channel.
    drop(writer);
    manager.destroy(&task_id).await;

    let mut events = vec![];
    while let Some(event) = reader.read().await {
        events.push(event.unwrap());
    }
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], StreamResponse::StatusUpdate(_)));
    assert!(matches!(&events[1], StreamResponse::ArtifactUpdate(_)));
}

#[tokio::test]
async fn queue_get_or_create_reuses_writer() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("task-3");

    // First call creates the queue.
    let (_writer1, reader1) = manager.get_or_create(&task_id).await;
    assert!(reader1.is_some());

    // Second call returns the same writer but no reader.
    let (_writer2, reader2) = manager.get_or_create(&task_id).await;
    assert!(reader2.is_none());
}

#[tokio::test]
async fn queue_destroy_allows_recreation() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("task-4");

    // Create and destroy.
    let (_writer, _reader) = manager.get_or_create(&task_id).await;
    manager.destroy(&task_id).await;

    // Recreate should give a new reader.
    let (_writer2, reader2) = manager.get_or_create(&task_id).await;
    assert!(reader2.is_some());
}

// ── SSE frame formatting tests ──────────────────────────────────────────────

#[test]
fn sse_write_event_format() {
    let frame = write_event("message", r#"{"status":"ok"}"#);
    let text = String::from_utf8(frame.to_vec()).expect("utf8");
    assert!(text.starts_with("event: message\n"));
    assert!(text.contains("data: {\"status\":\"ok\"}\n"));
    assert!(text.ends_with("\n\n"));
}

#[test]
fn sse_write_event_multiline() {
    let frame = write_event("message", "line1\nline2");
    let text = String::from_utf8(frame.to_vec()).expect("utf8");
    assert!(text.contains("data: line1\n"));
    assert!(text.contains("data: line2\n"));
}

#[test]
fn sse_write_keep_alive_format() {
    let frame = write_keep_alive();
    let text = String::from_utf8(frame.to_vec()).expect("utf8");
    assert_eq!(text, ": keep-alive\n\n");
}
