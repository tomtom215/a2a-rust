// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Retries and duplicates met while a send runs (audit E-F24).

use super::*;

// -- a retry of a send that runs ---------------------------------------------

/// A retry routed to another replica while the first send runs there is
/// answered with that send's task at once, not made to wait for a turn it
/// will not get and refused.
#[tokio::test]
async fn a_retry_on_another_replica_while_the_send_runs_is_replayed() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let release = Arc::new(Notify::new());
    let a = replica(
        &store,
        Gated::new(&claims),
        Parked(Arc::clone(&release)),
        Duration::from_secs(5),
    );
    let ran_on_b = Arc::new(AtomicUsize::new(0));
    let b = replica(
        &store,
        Gated::new(&claims),
        CountingExecutor(Arc::clone(&ran_on_b)),
        Duration::from_secs(5),
    );

    a.on_send_message(continuation("m1"), false, None)
        .await
        .expect("the send starts on A");
    let started = tokio::time::Instant::now();
    let retried = b
        .on_send_message(continuation("m1"), false, None)
        .await
        .expect("the retry is answered, not refused");
    assert_eq!(task_of(&retried).id, TaskId::new(TASK));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "answered at once and not after waiting out the turn: {:?}",
        started.elapsed()
    );
    assert_eq!(ran_on_b.load(Ordering::SeqCst), 0, "the retry ran nothing");
    release.notify_waiters();
}

/// The same, when the first send runs in this very process: its turn is not
/// "another replica's", and a retry of it is still told so rather than
/// refused as a send to a task already being processed.
#[tokio::test]
async fn a_retry_in_this_process_while_the_send_runs_is_replayed() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let release = Arc::new(Notify::new());
    let a = replica(
        &store,
        Gated::new(&claims),
        Parked(Arc::clone(&release)),
        Duration::from_secs(5),
    );

    a.on_send_message(continuation("m1"), false, None)
        .await
        .expect("the send starts");
    let retried = a
        .on_send_message(continuation("m1"), false, None)
        .await
        .expect("the retry is answered, not refused");
    assert_eq!(task_of(&retried).id, TaskId::new(TASK));
    release.notify_waiters();
}

/// A different message presenting the same key while the turn runs here is
/// a reused key, refused as one is when the turn is free; and a different
/// message with no key, finding the turn held, is refused as busy.
#[tokio::test]
async fn another_message_is_not_taken_for_the_retry() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let release = Arc::new(Notify::new());
    let a = replica(
        &store,
        Gated::new(&claims),
        Parked(Arc::clone(&release)),
        Duration::from_secs(5),
    );
    a.on_send_message(continuation("m1"), false, None)
        .await
        .expect("the send starts");

    let reused = a
        .on_send_message(continuation("m2"), false, None)
        .await
        .expect_err("a second message cannot take the first one's key");
    assert!(
        matches!(&reused, ServerError::InvalidParams(m) if m.contains("held by message `m1`")),
        "{reused:?}"
    );
    let mut unkeyed = continuation("m3");
    unkeyed.message.metadata = None;
    let asked = tokio::time::Instant::now();
    let busy = a
        .on_send_message(unkeyed, false, None)
        .await
        .expect_err("the turn is held");
    assert!(
        matches!(&busy, ServerError::UnsupportedOperation(m) if m.contains("already being processed")),
        "{busy:?}"
    );
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "a turn running in this process is refused at once, not after the wait: {:?}",
        asked.elapsed()
    );
    release.notify_waiters();
}

/// The turn is free and the key is held: the first send finished, and this is
/// its retry. The turn is taken first, the key then names the first send, and
/// the retry is answered with its task and runs nothing.
#[tokio::test]
async fn a_retry_of_a_send_that_finished_is_replayed_once_it_holds_the_turn() {
    let store = store_with_a_parked_task().await;
    store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new(TASK))
        .await
        .expect("the first send's key");
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        Gated::new(&Arc::new(InMemoryTurnClaims::new())),
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let retried = handler
        .on_send_message(continuation("m1"), false, None)
        .await
        .expect("the retry is answered");
    assert_eq!(task_of(&retried).id, TaskId::new(TASK));
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "the retry ran nothing"
    );
}

/// A key the send could never claim is refused before the turn is asked for:
/// nothing is claimed for a send that was never going to run.
#[tokio::test]
async fn a_malformed_key_is_refused_before_the_turn_is_asked_for() {
    let store = store_with_a_parked_task().await;
    let gated = Gated::new(&Arc::new(InMemoryTurnClaims::new()));
    let attempts = Arc::clone(&gated.attempts);
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::new(AtomicUsize::new(0))),
        Duration::from_secs(5),
    );
    let mut params = continuation("m1");
    params.message.metadata = Some(serde_json::json!({ IDEMPOTENCY_METADATA_KEY: "short" }));

    let refused = handler
        .on_send_message(params, false, None)
        .await
        .expect_err("the key is malformed");
    assert!(
        matches!(refused, ServerError::InvalidParams(_)),
        "{refused:?}"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        0,
        "no claim was asked for on behalf of a send that was never going to run"
    );
}

/// Two copies of one message arrive together, on two replicas, while the turn
/// is free: one takes the turn and the key and runs, and the other is answered
/// with that task, at its next poll of the key and not after the turn ends.
#[tokio::test]
async fn two_copies_of_one_message_arriving_together_run_it_once() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let release = Arc::new(Notify::new());
    let ran = Arc::new(AtomicUsize::new(0));
    let a = replica(
        &store,
        Gated::new(&claims),
        Counted(Arc::clone(&release), Arc::clone(&ran)),
        Duration::from_secs(5),
    );
    let b = replica(
        &store,
        Gated::new(&claims),
        Counted(Arc::clone(&release), Arc::clone(&ran)),
        Duration::from_secs(5),
    );

    let asked = tokio::time::Instant::now();
    let (first, second) = tokio::join!(
        a.on_send_message(continuation("m1"), false, None),
        b.on_send_message(continuation("m1"), false, None),
    );
    let first = first.expect("one copy is answered");
    let second = second.expect("the other copy is answered");
    assert_eq!(task_of(&first).id, TaskId::new(TASK));
    assert_eq!(task_of(&second).id, TaskId::new(TASK));
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "answered at the next poll of the key, not when the turn ends: {:?}",
        asked.elapsed()
    );
    // The executor starts after the send has answered. Wait for it, then give a
    // second one time to start: there is none.
    while ran.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(ran.load(Ordering::SeqCst), 1, "the message ran once");
    release.notify_waiters();
}
