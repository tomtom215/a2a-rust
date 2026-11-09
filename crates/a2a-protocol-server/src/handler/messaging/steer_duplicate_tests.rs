// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A steering message sent twice, and how a delivery is answered (ADR 0036).

use a2a_protocol_types::steering::{Delivery, Receipt};

use super::steer_tests::{Mode, inbox_len, inbox_of, rig, send, start, steering_to};
use crate::SendMessageResult;
use crate::error::ServerError;
use crate::steering::SteeringConfig;

// ── Duplicates and the idempotency extension ────────────────────────────────

#[tokio::test]
async fn a_retried_steering_message_with_a_key_is_delivered_once() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    let id = start(&rig).await;
    let mut params = steering_to(&id.0, "once");
    a2a_protocol_types::idempotency::set_key(&mut params.message, "0123456789abcdef0123").unwrap();

    let first = send(&rig.handler, params.clone()).await.expect("delivered");
    assert_eq!(
        Receipt::read(&first).unwrap(),
        Some(Receipt::new(Delivery::Inbox))
    );
    let retry = send(&rig.handler, params.clone()).await.expect("answered");
    assert_eq!(
        Receipt::read(&retry).unwrap(),
        Some(Receipt::new(Delivery::Replayed)),
        "the retry says nothing was delivered again"
    );
    assert_eq!(retry.id, id);
    assert_eq!(inbox_len(&rig, &id).await, 1, "delivered once");
    assert_eq!(rig.metrics.outcomes(), ["delivered", "replayed"]);

    // The same key on a different message is a reuse, never a retry.
    let mut reused = steering_to(&id.0, "another");
    a2a_protocol_types::idempotency::set_key(&mut reused.message, "0123456789abcdef0123").unwrap();
    let reused = send(&rig.handler, reused).await;
    assert!(
        matches!(reused, Err(ServerError::InvalidParams(_))),
        "{reused:?}"
    );
    assert_eq!(inbox_len(&rig, &id).await, 1);
    rig.probe.release.notify_one();
}

/// A refused delivery frees its key: the retry that follows delivers.
#[tokio::test]
async fn a_key_is_free_again_after_a_refusal() {
    let rig = rig(
        Mode::Block,
        Some(SteeringConfig::new().with_inbox_capacity(1)),
    );
    let id = start(&rig).await;
    send(&rig.handler, steering_to(&id.0, "fills it"))
        .await
        .expect("room");
    let mut params = steering_to(&id.0, "waits");
    a2a_protocol_types::idempotency::set_key(&mut params.message, "fedcba9876543210fedc").unwrap();
    let refused = send(&rig.handler, params.clone()).await;
    assert!(
        matches!(refused, Err(ServerError::Overloaded(_))),
        "{refused:?}"
    );
    let inbox = inbox_of(&rig, &id).await;
    assert!(inbox.try_recv().is_some(), "the executor reads one");
    let delivered = send(&rig.handler, params).await.expect("now it fits");
    assert_eq!(
        Receipt::read(&delivered).unwrap(),
        Some(Receipt::new(Delivery::Inbox)),
        "delivered, not replayed"
    );
    rig.probe.release.notify_one();
}

/// A key that the same message used for a different task is not an answer
/// for this one.
#[tokio::test]
async fn a_key_held_for_another_task_is_not_a_replay_of_this_one() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    let a = start(&rig).await;
    let b = start(&rig).await;
    let mut to_a = steering_to(&a.0, "x");
    a2a_protocol_types::idempotency::set_key(&mut to_a.message, "0123456789abcdef0123").unwrap();
    send(&rig.handler, to_a.clone())
        .await
        .expect("delivered to A");
    // The same message (same id), pointed at B.
    let mut to_b = to_a;
    to_b.message.task_id = Some(b.clone());
    let refused = send(&rig.handler, to_b).await;
    assert!(
        matches!(&refused, Err(ServerError::InvalidParams(text)) if text.contains(&a.0)),
        "{refused:?}"
    );
    assert_eq!(inbox_len(&rig, &b).await, 0);
    rig.probe.release.notify_one();
    rig.probe.release.notify_one();
}

// ── The response shapes ─────────────────────────────────────────────────────

/// A streaming send that steers is answered with the task and ends.
#[tokio::test]
async fn a_streaming_steering_message_is_answered_with_one_task_event() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    let id = start(&rig).await;
    let mut params = steering_to(&id.0, "x");
    params.configuration = None;
    let result = rig
        .handler
        .on_send_message(params, true, None)
        .await
        .expect("answered");
    let SendMessageResult::Stream(mut reader) = result else {
        panic!("expected a stream");
    };
    let first = next(&mut reader).await.expect("one event");
    match first {
        a2a_protocol_types::events::StreamResponse::Task(task) => {
            assert_eq!(task.id, id);
            assert_eq!(
                Receipt::read(&task).unwrap(),
                Some(Receipt::new(Delivery::Inbox))
            );
        }
        other => panic!("expected the task, got {other:?}"),
    }
    assert!(next(&mut reader).await.is_none(), "the stream ends");
    rig.probe.release.notify_one();
}

/// The next event of `reader`, or `None` at its end.
async fn next(
    reader: &mut crate::streaming::InMemoryQueueReader,
) -> Option<a2a_protocol_types::events::StreamResponse> {
    use crate::streaming::EventQueueReader;
    reader.read().await.and_then(Result::ok).map(|e| e.event)
}
