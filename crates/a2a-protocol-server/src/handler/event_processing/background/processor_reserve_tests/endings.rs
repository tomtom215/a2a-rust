// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A task's ending that no queued event carries is recorded in the outbox before it is logged (ADR 0037, audit H14); the rig and helpers are `processor_reserve_tests`'s.

use super::*;

/// A task ended on another replica (the reaper's ending) is recorded behind
/// the rows a stopped replica still has reserved for it, so a webhook sees the
/// event the dead replica had stored before the ending, not after it. A live
/// run (`scripts/live/outbox_live.py`) delivered `failed` 40 s before the
/// artifact when the reaper sent its ending from its own loop.
#[tokio::test(start_paused = true)]
async fn an_ending_recorded_after_a_crash_is_delivered_behind_the_reserved_event() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::After);
    let running = rig.run_on_a(artifact(1));
    parked(&rig).await;
    rig.kill_a(running).await;

    assert!(
        rig.b
            .record_now(&TaskId::new("t"), &status(2, TaskState::Failed).event)
            .await,
        "the handler has an outbox"
    );
    rig.wait(Duration::from_secs(30)).await;
    assert!(
        rig.b_seen.try_recv().is_err(),
        "the ending waits behind the reserved artifact"
    );

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: None
        },
        "the stored artifact first"
    );
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: Some(TaskState::Failed)
        },
        "then the ending"
    );
}

/// Without an outbox there is nothing to record in: the caller sends it.
#[tokio::test]
async fn without_an_outbox_nothing_is_recorded() {
    assert!(
        !crate::handler::PushLanes::default()
            .record_now(&TaskId::new("t"), &status(1, TaskState::Failed).event)
            .await
    );
}

/// A task's ending that no queued event carried (another replica cancelled
/// it) is recorded in the outbox before it is logged: a stop between the two
/// leaves the log without the ending, which is what the reaper looks for, and
/// the webhook told. (Logged first, a stop in between left a log that had the
/// ending and a webhook that was never told, and the reaper skips an ending
/// the log already holds.)
#[tokio::test(start_paused = true)]
async fn an_ending_is_recorded_before_it_is_logged() {
    let mut rig = Rig::new(TaskState::Canceled).await;
    rig.store.park_next_append();
    let (store, lanes, limits) = (Arc::clone(&rig.store), rig.a.clone(), rig.limits.clone());
    let running = tokio::spawn(async move {
        let id = TaskId::new("t");
        let mut run = Processor::new(
            &id,
            BackgroundDeps {
                task_store: &*store,
                push: &lanes,
                limits: &limits,
                metrics: &crate::metrics::NoopMetrics,
            },
            task(TaskState::Working),
            CancellationToken::new(),
            None,
            None,
        );
        run.close_out().await;
    });
    parked(&rig).await;

    assert_eq!(
        next(&mut rig.a_seen).await,
        Seen {
            replica: "a",
            state: Some(TaskState::Canceled)
        },
        "delivered while its log write has not happened"
    );
    assert!(
        rig.store
            .inner
            .read_events(&TaskId::new("t"), 0, 10)
            .await
            .unwrap()
            .is_empty(),
        "the log does not have it yet"
    );
    rig.kill_a(running).await;
}

/// The same for a verdict from another writer: a terminal event refused
/// because the task is already cancelled is answered with the stored state,
/// which is recorded in the outbox before it is written to the log.
#[tokio::test(start_paused = true)]
async fn a_verdict_is_recorded_before_it_is_logged() {
    let mut rig = Rig::new(TaskState::Canceled).await;
    rig.store.park_next_append();
    let running = rig.run_on_a(status(1, TaskState::Completed));
    parked(&rig).await;

    assert_eq!(
        next(&mut rig.a_seen).await,
        Seen {
            replica: "a",
            state: Some(TaskState::Canceled)
        },
        "the stored state, delivered while its log write has not happened"
    );
    assert!(
        rig.store
            .inner
            .read_events(&TaskId::new("t"), 0, 10)
            .await
            .unwrap()
            .is_empty(),
        "the log does not have it yet"
    );
    rig.kill_a(running).await;
}
