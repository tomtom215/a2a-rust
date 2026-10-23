// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §4.3.3 and §13.2: "Agents MAY stop attempting delivery after a
//! configured number of consecutive failures". `PushRetryPolicy::max_attempts`
//! is that number: a webhook that fails every attempt gets exactly as many
//! as configured, and the delivery is then reported failed.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_server::push::{HttpPushSender, PushRetryPolicy, PushSender};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A webhook that answers every request `503` and counts them. The count is
/// taken before the response is written, so it is final once the sender has
/// read the last response.
async fn failing_webhook() -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/webhook", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = stream.read(&mut buf).await;
                seen.fetch_add(1, Ordering::SeqCst);
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
            });
        }
    });
    (url, count)
}

fn event() -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("task-1"),
        context_id: ContextId::new("ctx"),
        status: TaskStatus::new(TaskState::Working),
        metadata: None,
    })
}

async fn attempts_made(max_attempts: usize) -> usize {
    let (url, count) = failing_webhook().await;
    let sender = HttpPushSender::new()
        .allow_private_urls()
        .with_retry_policy(
            PushRetryPolicy::default()
                .with_max_attempts(max_attempts)
                .with_backoff(vec![Duration::from_millis(1)]),
        );
    let result = sender
        .send(
            &url,
            &event(),
            &TaskPushNotificationConfig::new("task-1", &url),
        )
        .await;
    assert!(
        result.is_err(),
        "every attempt failed, so the delivery is reported failed: {result:?}"
    );
    count.load(Ordering::SeqCst)
}

#[tokio::test]
async fn push_delivery_stops_after_the_configured_number_of_failures() {
    // Neither is the default of 3, so a policy that ignored the setting, or
    // a loop that stopped early or late, fails one of them.
    assert_eq!(attempts_made(2).await, 2, "two attempts were configured");
    assert_eq!(attempts_made(5).await, 5, "five attempts were configured");
}
