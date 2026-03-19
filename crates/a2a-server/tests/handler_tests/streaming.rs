// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for event-queue streaming behaviour, verifying event ordering and
//! failure propagation through the stream.

use super::*;

#[tokio::test]
async fn streaming_events_arrive_in_order() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("test"), true, None)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected stream"),
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
        "status updates must arrive in order: Working then Completed"
    );
}

#[tokio::test]
async fn streaming_failure_produces_failed_event() {
    let handler = RequestHandlerBuilder::new(FailingExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("boom"), true, None)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected stream"),
    };

    let mut saw_failed = false;
    while let Some(event) = reader.read().await {
        if let Ok(StreamResponse::StatusUpdate(u)) = event {
            if u.status.state == TaskState::Failed {
                saw_failed = true;
            }
        }
    }

    assert!(saw_failed, "expected a Failed status update in stream");
}
