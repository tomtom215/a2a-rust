// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What bounds a steering message, what it is not, and a turn ending under it
//! (ADR 0036).

use std::sync::Arc;

use a2a_protocol_types::message::MessageId;
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::steering::{self, Delivery, Receipt};
use a2a_protocol_types::task::TaskState;

use super::steer_tests::{
    Agent, GUARD, Mode, inbox_len, inbox_of, message, rig, send, start, steering_to,
};
use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::steering::{Inbox, SteeringConfig};

// ── The bounds ──────────────────────────────────────────────────────────────

/// A full inbox refuses with 503 and `Retry-After`, with the executor
/// blocked: the call returns, nothing already queued is lost, and the refused
/// message is not queued behind them.
#[tokio::test]
async fn a_full_inbox_answers_503_with_the_executor_blocked() {
    let rig = rig(
        Mode::Block,
        Some(SteeringConfig::new().with_inbox_capacity(2)),
    );
    let id = start(&rig).await;
    send(&rig.handler, steering_to(&id.0, "one"))
        .await
        .expect("room");
    send(&rig.handler, steering_to(&id.0, "two"))
        .await
        .expect("room");

    let refused = tokio::time::timeout(GUARD, send(&rig.handler, steering_to(&id.0, "three")))
        .await
        .expect("a full inbox does not block the caller");
    let error = refused.expect_err("the third is refused");
    assert!(matches!(error, ServerError::Overloaded(_)), "{error:?}");
    assert_eq!(error.http_status(), 503);
    assert!(error.retry_after().is_some(), "{error:?}");
    assert!(
        error.to_string().contains("holds its 2 messages"),
        "the refusal says why: {error}"
    );
    assert_eq!(rig.metrics.outcomes(), ["delivered", "delivered", "full"]);
    let inbox = inbox_of(&rig, &id).await;
    let held: Vec<String> = std::iter::from_fn(|| inbox.try_recv())
        .map(|s| s.message.text().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(held, ["one", "two"], "nothing queued was lost or reordered");
    rig.probe.release.notify_one();
}

/// A flood of steering messages is bounded by the inbox and answered 503
/// each time past it, without a single one being dropped silently.
#[tokio::test]
async fn a_flood_is_bounded_by_the_inbox_and_every_message_is_answered() {
    let rig = rig(
        Mode::Block,
        Some(SteeringConfig::new().with_inbox_capacity(3)),
    );
    let id = start(&rig).await;
    let mut delivered = 0;
    let mut refused = 0;
    for i in 0..25 {
        match send(&rig.handler, steering_to(&id.0, &format!("m{i}"))).await {
            Ok(task) => {
                assert_eq!(
                    Receipt::read(&task).unwrap(),
                    Some(Receipt::new(Delivery::Inbox))
                );
                delivered += 1;
            }
            Err(ServerError::Overloaded(_)) => refused += 1,
            Err(other) => panic!("{other:?}"),
        }
    }
    assert_eq!((delivered, refused), (3, 22));
    assert_eq!(inbox_len(&rig, &id).await, 3);
    assert_eq!(rig.metrics.count("delivered"), 3);
    assert_eq!(rig.metrics.count("full"), 22);
    rig.probe.release.notify_one();
}

/// One byte under, at and one byte over the size limit, measured on the
/// message as it serializes.
#[tokio::test]
async fn the_message_size_limit_is_exact() {
    // Task ids and message ids are UUIDs, 36 bytes, so a stand-in of that
    // length has the overhead of the real thing.
    let probe = message(Some(&"t".repeat(36)), "");
    let overhead = {
        let mut m = probe.clone();
        steering::declare(&mut m);
        serde_json::to_vec(&m).unwrap().len()
    };
    let limit = overhead + 40;
    let rig = rig(
        Mode::Block,
        Some(SteeringConfig::new().with_max_message_size(limit)),
    );
    let id = start(&rig).await;
    let of_size = |bytes: usize| {
        let mut m = message(Some(&id.0), &"x".repeat(bytes - overhead));
        m.id = MessageId::new(probe.id.0.clone());
        steering::declare(&mut m);
        assert_eq!(
            serde_json::to_vec(&m).unwrap().len(),
            bytes,
            "sized as asked"
        );
        MessageSendParams::new(m)
    };

    send(&rig.handler, of_size(limit - 1))
        .await
        .expect("one under");
    send(&rig.handler, of_size(limit))
        .await
        .expect("at the limit");
    let over = send(&rig.handler, of_size(limit + 1)).await;
    match over {
        Err(ServerError::PayloadTooLarge(text)) => {
            assert_eq!(
                text,
                format!(
                    "a steering message is {} bytes, over this server's limit of {limit} bytes",
                    limit + 1
                )
            );
        }
        other => panic!("expected PayloadTooLarge, got {other:?}"),
    }
    assert_eq!(
        inbox_len(&rig, &id).await,
        2,
        "the oversize message was not queued"
    );
    assert_eq!(
        rig.metrics.outcomes(),
        ["delivered", "delivered", "too_large"]
    );
    assert_eq!(
        send(&rig.handler, of_size(limit + 1))
            .await
            .expect_err("still refused")
            .http_status(),
        413
    );
    rig.probe.release.notify_one();
}

#[test]
fn building_refuses_a_steering_config_that_could_never_deliver() {
    for config in [
        SteeringConfig::new().with_inbox_capacity(0),
        SteeringConfig::new().with_max_message_size(0),
    ] {
        let built = RequestHandlerBuilder::new(Agent {
            mode: Mode::Block,
            probe: Arc::default(),
        })
        .without_agent_card()
        .with_steering(config)
        .build();
        assert!(
            matches!(&built, Err(ServerError::Config(why)) if why.contains("at least 1")),
            "{config:?}: {:?}",
            built.as_ref().err()
        );
    }
    let ok = RequestHandlerBuilder::new(Agent {
        mode: Mode::Block,
        probe: Arc::default(),
    })
    .without_agent_card()
    .with_steering(
        SteeringConfig::new()
            .with_inbox_capacity(1)
            .with_max_message_size(1),
    )
    .build();
    assert!(ok.is_ok(), "a bound of 1 is a bound");
}

// ── What it is not ──────────────────────────────────────────────────────────

/// A steering message does not cancel, whatever it says: the task is still
/// working and its token is not fired.
#[tokio::test]
async fn a_steering_message_does_not_cancel() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    let id = start(&rig).await;
    send(&rig.handler, steering_to(&id.0, "cancel this task now"))
        .await
        .expect("delivered");
    let tokens = rig.handler.cancellation_tokens.read().await;
    assert!(
        !tokens.get(&id).expect("registered").token.is_cancelled(),
        "the token was not fired"
    );
    drop(tokens);
    let stored = rig.handler.task_store.get(&id).await.unwrap().unwrap();
    assert!(
        matches!(
            stored.status.state,
            TaskState::Working | TaskState::Submitted
        ),
        "{:?}",
        stored.status.state
    );
    rig.probe.release.notify_one();
}

/// The stored task is not touched by a delivery: the history the running
/// turn writes is its own, and nothing here can lose an update of it.
#[tokio::test]
async fn a_delivery_writes_nothing_to_the_stored_task() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    let id = start(&rig).await;
    let before = rig.handler.task_store.get(&id).await.unwrap().unwrap();
    let answer = send(&rig.handler, steering_to(&id.0, "hello"))
        .await
        .unwrap();
    let after = rig.handler.task_store.get(&id).await.unwrap().unwrap();
    assert_eq!(before.history, after.history);
    assert_eq!(before.metadata, after.metadata);
    assert!(Receipt::read(&answer).unwrap().is_some());
    assert!(Receipt::read(&after).unwrap().is_none());
    rig.probe.release.notify_one();
}

// ── Halt, tenants, the turn ending ──────────────────────────────────────────

/// A halted server takes no steering: whatever else it stopped, a message
/// sent after the halt is refused as halted and delivers nothing.
#[tokio::test]
async fn a_halted_server_takes_no_steering() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    let id = start(&rig).await;
    let inbox = inbox_of(&rig, &id).await;
    let _ = rig
        .handler
        .halt(crate::HaltScope::All, "operator", "test")
        .await;
    let refused = send(&rig.handler, steering_to(&id.0, "x")).await;
    assert!(
        matches!(refused, Err(ServerError::Halted(_))),
        "{refused:?}"
    );
    assert!(inbox.is_empty(), "nothing was delivered");
}

/// A turn is registered in the map with an inbox of its own, on a handler
/// built to take steering: a unit check of the entry's rules, one at a time.
#[tokio::test]
async fn only_a_live_unparked_turn_of_this_tenant_can_be_steered() {
    use crate::handler::{CancellationEntry, ExecutorTurn};
    let entry = |tenant: &str| CancellationEntry {
        token: tokio_util::sync::CancellationToken::new(),
        created_at: std::time::Instant::now(),
        turn: Arc::new(ExecutorTurn::with_inbox(Some(Arc::new(Inbox::new(1))))),
        tenant: tenant.to_owned(),
        lineage: a2a_protocol_types::lineage::Lineage::origin("c"),
    };
    assert!(super::steer::steerable(&entry("acme"), "acme"));
    assert!(
        !super::steer::steerable(&entry("acme"), "other"),
        "another tenant's"
    );
    assert!(
        !super::steer::steerable(&entry("acme"), ""),
        "no tenant is not acme"
    );
    assert!(super::steer::steerable(&entry(""), ""));
    let cancelled = entry("acme");
    cancelled.token.cancel();
    assert!(
        !super::steer::steerable(&cancelled, "acme"),
        "a cancelled turn"
    );
    let parked = entry("acme");
    parked
        .turn
        .parked
        .store(true, std::sync::atomic::Ordering::Release);
    assert!(!super::steer::steerable(&parked, "acme"), "a parked turn");
}

#[test]
fn only_a_working_or_submitted_task_is_running() {
    for state in [TaskState::Working, TaskState::Submitted] {
        assert!(super::steer::running_state(state), "{state:?}");
    }
    for state in [
        TaskState::InputRequired,
        TaskState::AuthRequired,
        TaskState::Completed,
        TaskState::Failed,
        TaskState::Canceled,
        TaskState::Rejected,
    ] {
        assert!(!super::steer::running_state(state), "{state:?}");
    }
}

/// A message that finds the turn's inbox closed — the executor has returned
/// and the turn is settling — is refused, not accepted and dropped, and its
/// idempotency key is free again.
#[tokio::test]
async fn a_message_that_finds_the_turn_ended_is_refused_and_not_counted_delivered() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    let id = start(&rig).await;
    let inbox = inbox_of(&rig, &id).await;
    assert_eq!(inbox.close(), 0, "the turn's executor has returned");

    let mut params = steering_to(&id.0, "too late");
    a2a_protocol_types::idempotency::set_key(&mut params.message, "0123456789abcdef0123").unwrap();
    let refused = send(&rig.handler, params.clone()).await;
    match &refused {
        Err(error @ ServerError::Overloaded(text)) => {
            assert!(
                text.contains("ended before the steering message was delivered"),
                "{text}"
            );
            assert_eq!(error.http_status(), 503);
            assert!(error.retry_after().is_some());
        }
        other => panic!("expected a transient refusal, got {other:?}"),
    }
    assert_eq!(rig.metrics.outcomes(), ["ended"]);
    // The key was released: the retry is refused the same way, not replayed
    // as delivered.
    let again = send(&rig.handler, params).await;
    assert!(
        matches!(again, Err(ServerError::Overloaded(_))),
        "{again:?}"
    );
    assert_eq!(rig.metrics.outcomes(), ["ended", "ended"]);
    rig.probe.release.notify_one();
}
