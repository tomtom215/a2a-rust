// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A send waiting for another replica's turn does not hold up its context
//! (audit E-F23).

use super::*;

// -- the per-context lock (audit E-F23) ---------------------------------------

/// A send waiting for another replica's turn does not hold the lock of its
/// context: a send to another task of the same context goes on meanwhile.
/// Held across the wait, it kept the sibling waiting for the whole of it.
#[tokio::test(start_paused = true)]
async fn a_send_waiting_for_a_turn_does_not_hold_up_its_context() {
    let store = store_with_a_parked_task().await;
    store
        .save(&Task::new(
            TaskId::new("t-sibling"),
            ContextId::new("ctx-1"),
            TaskStatus::new(TaskState::InputRequired),
        ))
        .await
        .expect("the sibling saves");
    let claims = Arc::new(InMemoryTurnClaims::new());
    let mut gated = Gated::new(&claims);
    gated.elsewhere.store(true, Ordering::SeqCst);
    gated.only_task = Some(TASK.to_owned());
    let elsewhere = Arc::clone(&gated.elsewhere);
    let attempts = Arc::clone(&gated.attempts);
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let waiting = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move {
            handler
                .on_send_message(continuation("m1"), false, None)
                .await
        }
    });
    while attempts.load(Ordering::SeqCst) < 2 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The first send is in its wait. A send to the sibling is not held up
    // behind it.
    let mut sibling = continuation("m2");
    sibling.message.task_id = Some(TaskId::new("t-sibling"));
    sibling.message.metadata = None;
    let sent = tokio::time::timeout(
        Duration::from_secs(1),
        handler.on_send_message(sibling, false, None),
    )
    .await
    .expect("the sibling send waited behind the send that waits for another replica")
    .expect("the sibling send runs");
    assert_eq!(task_of(&sent).id, TaskId::new("t-sibling"));

    // The turn frees, and the waiting send goes on.
    elsewhere.store(false, Ordering::SeqCst);
    let answered = waiting
        .await
        .expect("the send task ends")
        .expect("the waiting send runs once it holds the turn");
    assert_eq!(task_of(&answered).id, TaskId::new(TASK));
    assert_eq!(executions.load(Ordering::SeqCst), 2, "both sends ran");
}
