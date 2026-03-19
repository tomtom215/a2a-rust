// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Streaming mode tests.
//!
//! These tests verify that the `RequestHandler` correctly delivers events
//! through the streaming (SSE) interface, including status updates, artifact
//! events, error-to-failed mapping, total event counts, message event
//! passthrough, and task snapshot passthrough.

use super::*;

#[tokio::test]
async fn streaming_mode_delivers_status_events() {
    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming message");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut states = vec![];
    while let Some(event) = reader.read().await {
        if let Ok(StreamResponse::StatusUpdate(u)) = event {
            states.push(u.status.state);
        }
    }

    assert_eq!(
        states,
        vec![TaskState::Working, TaskState::Completed],
        "stream should deliver Working then Completed status events in order"
    );
}

#[tokio::test]
async fn streaming_mode_delivers_artifact_events() {
    let handler = RequestHandlerBuilder::new(ArtifactExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming message");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut artifact_count = 0;
    let mut states = vec![];
    while let Some(event) = reader.read().await {
        match event {
            Ok(StreamResponse::ArtifactUpdate(_)) => artifact_count += 1,
            Ok(StreamResponse::StatusUpdate(u)) => states.push(u.status.state),
            _ => {}
        }
    }

    assert_eq!(artifact_count, 1, "should receive exactly 1 artifact event");
    assert_eq!(
        states,
        vec![TaskState::Working, TaskState::Completed],
        "stream should deliver Working then Completed status events in order"
    );
}

#[tokio::test]
async fn streaming_mode_error_produces_failed_event() {
    let handler = RequestHandlerBuilder::new(ErrorExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming message");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut saw_failed = false;
    while let Some(event) = reader.read().await {
        if let Ok(StreamResponse::StatusUpdate(u)) = event {
            if u.status.state == TaskState::Failed {
                saw_failed = true;
            }
        }
    }

    assert!(saw_failed, "should see Failed status event in stream");
}

#[tokio::test]
async fn streaming_mode_receives_all_events() {
    let handler = RequestHandlerBuilder::new(ArtifactExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming message");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut event_count = 0;
    while let Some(event) = reader.read().await {
        if event.is_ok() {
            event_count += 1;
        }
    }
    assert_eq!(
        event_count, 3,
        "should receive exactly 3 events (Working + Artifact + Completed), got {event_count}"
    );
}

#[tokio::test]
async fn streaming_mode_message_event_passes_through() {
    let handler = RequestHandlerBuilder::new(MessageEventExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut saw_message = false;
    let mut states = vec![];
    while let Some(event) = reader.read().await {
        match event {
            Ok(StreamResponse::Message(_)) => saw_message = true,
            Ok(StreamResponse::StatusUpdate(u)) => states.push(u.status.state),
            _ => {}
        }
    }

    assert!(saw_message, "should have seen Message event in stream");
    assert_eq!(states, vec![TaskState::Working, TaskState::Completed]);
}

#[tokio::test]
async fn streaming_mode_task_snapshot_in_stream() {
    let handler = RequestHandlerBuilder::new(TaskEventExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut saw_task = false;
    while let Some(event) = reader.read().await {
        if let Ok(StreamResponse::Task(_)) = event {
            saw_task = true;
        }
    }

    assert!(saw_task, "should have seen Task snapshot in stream");
}
