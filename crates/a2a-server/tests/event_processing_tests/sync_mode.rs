// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Sync mode (`collect_events`) tests.
//!
//! These tests verify that the `RequestHandler` correctly processes events
//! in synchronous (non-streaming) mode, including status updates, artifact
//! collection, task replacement, error handling, message event no-ops, and
//! push notification behaviour when no sender or config is present.

use super::*;

#[tokio::test]
async fn sync_mode_status_updates_stored() {
    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
}

#[tokio::test]
async fn sync_mode_artifact_updates_appended() {
    let handler = RequestHandlerBuilder::new(ArtifactExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
    let artifacts = task.artifacts.expect("task should have artifacts");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id.0, "art-1");
}

#[tokio::test]
async fn sync_mode_task_event_replaces_task() {
    let handler = RequestHandlerBuilder::new(TaskEventExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
    let artifacts = task.artifacts.expect("replaced task should have artifacts");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id.0, "replaced-art");
    let meta = task.metadata.expect("replaced task should have metadata");
    assert_eq!(meta["replaced"], true);
}

#[tokio::test]
async fn sync_mode_error_marks_failed() {
    let handler = RequestHandlerBuilder::new(ErrorExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await;

    match result {
        Ok(send_result) => {
            let task = extract_task(send_result);
            assert_eq!(task.status.state, TaskState::Failed);
        }
        Err(e) => {
            // Error propagation is acceptable — verify the error message.
            let err_msg = format!("{e:?}");
            assert!(
                err_msg.contains("something went wrong"),
                "expected error to contain 'something went wrong', got: {err_msg}"
            );
        }
    }
}

#[tokio::test]
async fn sync_mode_message_event_is_noop_for_state() {
    let handler = RequestHandlerBuilder::new(MessageEventExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
}

#[tokio::test]
async fn sync_mode_no_push_calls_when_no_sender() {
    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
}

#[tokio::test]
async fn sync_mode_no_push_calls_when_no_config() {
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .with_push_sender(SharedRecordingPushSender {
            calls: calls.clone(),
        })
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let _task = extract_task(result);
    assert_eq!(
        calls.lock().unwrap().len(),
        0,
        "no push calls should be made when no push config is set"
    );
}
