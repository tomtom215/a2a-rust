// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The turn is claimed before the key: the window a key used to be held in,
//! and a send whose retry took the turn first (audit E-F24).

use super::*;

// -- the window --------------------------------------------------------------

/// The moment the send's first claim attempt is in flight, its key is free:
/// a retry arriving then has nothing to be answered by. Before, the key was
/// claimed first, so it was already held here.
#[tokio::test(start_paused = true)]
async fn a_send_asking_for_its_turn_holds_no_key() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let pause = Arc::new(Pause::default());
    let mut gated = Gated::new(&claims);
    gated.pause = Some(Arc::clone(&pause));
    gated.elsewhere.store(true, Ordering::SeqCst);
    let elsewhere = Arc::clone(&gated.elsewhere);
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let sent = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move {
            handler
                .on_send_message(continuation("m1"), false, None)
                .await
        }
    });
    pause.entered.notified().await;
    assert_eq!(
        holder(&store).await,
        None,
        "the first claim attempt is in flight and the key is not held"
    );

    // Another replica's turn ends, and the send takes the turn and then the key.
    pause.proceed.notify_one();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        holder(&store).await,
        None,
        "still waiting, still no key (the turn is held elsewhere)"
    );
    elsewhere.store(false, Ordering::SeqCst);
    let answered = sent
        .await
        .expect("the send task ends")
        .expect("the send runs once it holds the turn");
    assert_eq!(task_of(&answered).id, TaskId::new(TASK));
    assert_eq!(
        holder(&store).await,
        Some(IdempotencyHolder::new(
            MessageId::new("m1"),
            TaskId::new(TASK)
        )),
        "the key is taken once the turn is held"
    );
}

/// The task finished between the send's first read and its claim: the send is
/// refused, and the key it had just taken is released, so the caller's retry
/// is not answered with a task that was never continued.
#[tokio::test]
async fn a_send_that_finds_its_task_finished_under_the_claim_releases_its_key() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let mut gated = Gated::new(&claims);
    gated.finish_on_grant = Some(Arc::clone(&store));
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let refused = handler
        .on_send_message(continuation("m1"), false, None)
        .await
        .expect_err("the task finished before the send held the turn");
    assert!(
        matches!(&refused, ServerError::UnsupportedOperation(m) if m.contains("terminal state")),
        "{refused:?}"
    );
    assert_eq!(
        holder(&store).await,
        None,
        "the refused send released its key"
    );
    assert_eq!(executions.load(Ordering::SeqCst), 0, "nothing ran");
}

// -- a send that waits while its retry runs ----------------------------------

/// The retry took the turn first, and with it the key: the send that was
/// waiting is answered with the retry's task and runs nothing, well before
/// its own wait would have run out.
#[tokio::test(start_paused = true)]
async fn a_waiting_send_is_answered_when_its_retry_takes_the_key() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let gated = Gated::new(&claims);
    gated.elsewhere.store(true, Ordering::SeqCst);
    let attempts = Arc::clone(&gated.attempts);
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let started = tokio::time::Instant::now();
    let sent = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move {
            handler
                .on_send_message(continuation("m1"), false, None)
                .await
        }
    });
    while attempts.load(Ordering::SeqCst) < 3 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The retry, on another replica, won the turn and took the key.
    let taken = store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new(TASK))
        .await
        .expect("the key claims");
    assert_eq!(taken, IdempotencyClaim::Claimed);

    let answered = sent
        .await
        .expect("the send task ends")
        .expect("the waiting send is answered with the retry's task");
    assert_eq!(task_of(&answered).id, TaskId::new(TASK));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "answered when the key was taken, not when the wait ran out: {:?}",
        started.elapsed()
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "the waiting send ran nothing"
    );
}
