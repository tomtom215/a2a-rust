// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Two consequences the spec states without a keyword, and no ledger entry
//! tested:
//!
//! - §3.3.1: "Cancel Task operations are idempotent - multiple cancellation
//!   requests have the same effect." The SDK's reading: the effect is the
//!   task's state, which a second cancel leaves exactly as the first left
//!   it; the second answer is `TaskNotCancelableError`, which §3.1.5 lists
//!   for a task "already ... canceled" (§3.3.1's `TaskNotFoundError` is for
//!   one already purged).
//! - §3.1.10: "No further notifications will be sent to the configured
//!   webhook after deletion."

use a2a_protocol_server::error::ServerError;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_types::params::{CancelTaskParams, DeletePushConfigParams, TaskQueryParams};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;

use crate::s3_agent::{immediately, send};
use crate::stream_and_push::{Recorder, handler};

fn cancel(id: &str) -> CancelTaskParams {
    CancelTaskParams {
        tenant: None,
        id: id.to_owned(),
        metadata: None,
    }
}

#[tokio::test]
async fn a_second_cancel_has_the_same_effect_as_the_first() {
    let (handler, _seen) = handler(Recorder::default());
    let id = match handler
        .on_send_message(immediately(send("m-1", "wait", None, None)), false, None)
        .await
        .expect("send")
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    };
    let first = handler
        .on_cancel_task(cancel(&id), None)
        .await
        .expect("cancel");
    assert_eq!(first.status.state, TaskState::Canceled);
    let after_first = handler
        .on_get_task(TaskQueryParams::new(&id), None)
        .await
        .expect("get");

    let second = handler.on_cancel_task(cancel(&id), None).await;
    assert!(
        matches!(&second, Err(ServerError::TaskNotCancelable(t)) if t.0 == id),
        "{second:?}"
    );
    let after_second = handler
        .on_get_task(TaskQueryParams::new(&id), None)
        .await
        .expect("get");
    // The same effect: the stored task is unchanged, status timestamp and
    // history included.
    assert_eq!(
        serde_json::to_value(&after_second).expect("json"),
        serde_json::to_value(&after_first).expect("json")
    );
}

#[tokio::test]
async fn no_notification_reaches_a_webhook_after_its_config_is_deleted() {
    let push = Recorder::default();
    let (handler, seen) = handler(push.clone());
    let id = match handler
        .on_send_message(immediately(send("m-1", "wait", None, None)), false, None)
        .await
        .expect("send")
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    };
    let (deleted, kept) = (
        "https://hook.example.com/deleted",
        "https://hook.example.com/kept",
    );
    let mut config_ids = Vec::new();
    for url in [deleted, kept] {
        let made = handler
            .on_set_push_config(TaskPushNotificationConfig::new(id.as_str(), url), None)
            .await
            .expect("create");
        config_ids.push(made.id.expect("id"));
    }
    handler
        .on_delete_push_config(
            DeletePushConfigParams {
                tenant: None,
                task_id: id.clone(),
                id: config_ids[0].clone(),
            },
            None,
        )
        .await
        .expect("delete");

    // The agent finishes only now, so its terminal update is generated after
    // the deletion. (A `WORKING` delivery listed before the deletion may
    // still be in flight; what the spec rules out is a notification for
    // anything that happens afterwards.)
    seen.release.notify_one();
    // The kept config receives it: deliveries are running.
    push.wait_for(kept, "status:TASK_STATE_COMPLETED").await;
    let to_deleted: Vec<String> = push
        .deliveries()
        .into_iter()
        .filter(|(url, _)| url == deleted)
        .map(|(_, event)| event)
        .collect();
    assert!(
        !to_deleted.iter().any(|e| e.contains("COMPLETED")),
        "delivered after deletion: {to_deleted:?}"
    );
}
