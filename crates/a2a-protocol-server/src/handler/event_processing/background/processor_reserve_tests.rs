// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An event's push rows are in the outbox before the event is stored, so a
//! process that stops at any point leaves rows another replica settles from
//! the store (ADR 0037, audit H14).
//!
//! "Stops" is a task aborted at a chosen point of the store write: the
//! processor's future is dropped there, as a killed process drops it, and
//! nothing after that point runs. The clock is paused; a hold passes as soon
//! as nothing else can run.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{TaskId, TaskState};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::tests::{artifact, status, task};
use super::*;
use crate::handler::limits::HandlerLimits;
use crate::metrics::{Metrics, push_outcome};
use crate::push::outbox::{InMemoryPushOutbox, PushOutbox};
use crate::push::{InMemoryPushConfigStore, PushConfigStore, PushSender};
use crate::store::TaskStore;

use super::super::reserve_testing::{Outcomes, Seen, Sender, Stop, Stopping};

/// Two replicas over one task store, push config store and outbox.
struct Rig {
    store: Arc<Stopping>,
    outbox: Arc<InMemoryPushOutbox>,
    limits: HandlerLimits,
    metrics: Arc<Outcomes>,
    /// The replica that runs the task.
    a: crate::handler::PushLanes,
    a_stops: CancellationToken,
    a_seen: mpsc::UnboundedReceiver<Seen>,
    /// The replica that takes over.
    b: crate::handler::PushLanes,
    b_seen: mpsc::UnboundedReceiver<Seen>,
}

impl Rig {
    async fn new(stored: TaskState) -> Self {
        let store = Stopping::new();
        store.inner.save(&task(stored)).await.expect("seed");
        let configs = Arc::new(InMemoryPushConfigStore::new());
        configs
            .set(TaskPushNotificationConfig::new("t", "https://example.com/hook").with_id("cfg"))
            .await
            .expect("config");
        let outbox = Arc::new(InMemoryPushOutbox::new());
        let limits = HandlerLimits::default().with_store_write_retry(Duration::from_secs(1));
        let metrics = Arc::new(Outcomes::default());
        let lanes = |replica: &'static str, stops: CancellationToken| {
            let (seen, rx) = mpsc::unbounded_channel();
            let lanes = crate::handler::PushLanes::with_reservations(
                Arc::clone(&configs) as Arc<dyn PushConfigStore>,
                Some(Arc::new(Sender { replica, seen }) as Arc<dyn PushSender>),
                limits.clone(),
                Arc::clone(&metrics) as Arc<dyn Metrics>,
                TaskTracker::new(),
                Some((Arc::clone(&outbox) as Arc<dyn PushOutbox>, stops)),
                Some(Arc::clone(&store) as Arc<dyn TaskStore>),
            );
            (lanes, rx)
        };
        let a_stops = CancellationToken::new();
        let (a, a_seen) = lanes("a", a_stops.clone());
        let (b, b_seen) = lanes("b", CancellationToken::new());
        Self {
            store,
            outbox,
            limits,
            metrics,
            a,
            a_stops,
            a_seen,
            b,
            b_seen,
        }
    }

    /// Replica `a` handles `event`, on a task of its own so it can be stopped.
    fn run_on_a(&self, event: StreamEvent) -> tokio::task::JoinHandle<()> {
        let (store, lanes, limits) = (Arc::clone(&self.store), self.a.clone(), self.limits.clone());
        tokio::spawn(async move {
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
            run.handle(Ok(event)).await;
        })
    }

    /// Replica `a` stops: its task is aborted and its worker ends.
    async fn kill_a(&self, running: tokio::task::JoinHandle<()>) {
        running.abort();
        let _ = running.await;
        self.a_stops.cancel();
    }

    async fn rows(&self) -> usize {
        self.outbox.pending().await.unwrap()
    }

    /// Lets `d` pass on the paused clock, with every worker running.
    async fn wait(&self, d: Duration) {
        tokio::time::sleep(d).await;
    }
}

/// The next delivery on `rx`. The clock is paused, so a delivery that never
/// comes would otherwise be waited for for ever, with the workers' polls
/// advancing the clock: bounded, it fails.
async fn next(rx: &mut mpsc::UnboundedReceiver<Seen>) -> Seen {
    tokio::time::timeout(Duration::from_mins(15), rx.recv())
        .await
        .expect("no delivery within 15 minutes of (paused) time")
        .expect("the sender is gone")
}

/// The write that is to stop has reached its stop.
async fn parked(rig: &Rig) {
    tokio::time::timeout(Duration::from_mins(15), rig.store.parked.notified())
        .await
        .expect("the write never reached its stop");
}

/// The hold the rig's limits give a reservation.
fn hold(rig: &Rig) -> Duration {
    super::super::push_delivery::reservation_hold(&rig.limits)
}

/// An event that is stored is delivered once, from the row reserved for it:
/// the lane records nothing of its own.
#[tokio::test(start_paused = true)]
async fn a_stored_event_is_delivered_once_from_its_reservation() {
    let mut rig = Rig::new(TaskState::Working).await;
    let started = tokio::time::Instant::now();
    rig.run_on_a(status(1, TaskState::Working)).await.unwrap();

    assert_eq!(
        next(&mut rig.a_seen).await,
        Seen {
            replica: "a",
            state: Some(TaskState::Working)
        }
    );
    // Armed, and the worker woken, when the event was stored: no timer ran. A
    // worker left to its poll (a second) or to the hold (minutes) takes longer.
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "delivered {:?} after the event was stored (the hold is {:?})",
        started.elapsed(),
        hold(&rig)
    );
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err(), "delivered once");
    assert!(rig.b_seen.try_recv().is_err());
    assert_eq!(rig.rows().await, 0);
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 0);
}

/// A store that refuses the event (the task is already finished) leaves no row
/// for it; the one delivery is the verdict the processor pushes.
#[tokio::test(start_paused = true)]
async fn an_event_the_store_refused_leaves_no_row_of_its_own() {
    let mut rig = Rig::new(TaskState::Canceled).await;
    rig.run_on_a(artifact(1)).await.unwrap();

    assert_eq!(
        next(&mut rig.a_seen).await,
        Seen {
            replica: "a",
            state: Some(TaskState::Canceled)
        },
        "the verdict, not the artifact"
    );
    assert!(
        hold(&rig) > Duration::from_secs(40),
        "the wait below ends before the hold does"
    );
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err(), "nothing else was delivered");
    assert_eq!(
        rig.rows().await,
        0,
        "the artifact's reservation was released by its processor, not settled by the hold"
    );
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 0);
}

/// The window H14 is about: the replica stops after the store has the event and
/// before it arms the rows. Another replica arms them, from the store, and
/// delivers.
#[tokio::test(start_paused = true)]
async fn a_stop_after_the_store_write_is_settled_by_another_replica_and_delivered() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::After);
    let running = rig.run_on_a(status(1, TaskState::Working));
    parked(&rig).await;
    rig.kill_a(running).await;

    assert_eq!(rig.rows().await, 1, "the reservation outlived its replica");
    rig.wait(Duration::from_secs(5)).await;
    assert!(rig.b_seen.try_recv().is_err(), "not armed yet, so not sent");

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: Some(TaskState::Working)
        }
    );
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.b_seen.try_recv().is_err(), "once");
    assert!(
        rig.a_seen.try_recv().is_err(),
        "the stopped replica sent none"
    );
    assert_eq!(rig.rows().await, 0);
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 1);
}

/// The replica stops before the store has the event: no webhook is told of an
/// event the task does not have. (The log has it: it is written first.)
#[tokio::test(start_paused = true)]
async fn a_stop_before_the_store_write_is_settled_as_not_stored_and_tells_nobody() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::Before);
    let running = rig.run_on_a(status(1, TaskState::Working));
    parked(&rig).await;
    rig.kill_a(running).await;
    let id = TaskId::new("t");
    assert_eq!(
        rig.store.inner.read_events(&id, 0, 10).await.unwrap().len(),
        1,
        "the event is in the log, as it is written before the fold"
    );

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(rig.rows().await, 0, "released");
    assert!(rig.a_seen.try_recv().is_err());
    assert!(rig.b_seen.try_recv().is_err(), "an event the task lacks");
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 1);
}

/// An artifact update is reserved like a status update: the same stop, the
/// same recovery.
#[tokio::test(start_paused = true)]
async fn an_artifact_update_is_reserved_too() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::After);
    let running = rig.run_on_a(artifact(1));
    parked(&rig).await;
    rig.kill_a(running).await;
    assert_eq!(rig.rows().await, 1);

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: None
        },
        "an artifact update"
    );
    assert_eq!(rig.rows().await, 0);
}

/// The hold outlasts a fold however long the store is made to retry: two
/// retried writes (the log, then the save) and a margin.
#[test]
fn the_hold_outlasts_two_store_write_retries_and_a_margin() {
    for retry in [
        Duration::from_secs(1),
        Duration::from_secs(30),
        Duration::from_secs(600),
    ] {
        let limits = HandlerLimits::default().with_store_write_retry(retry);
        let held = super::super::push_delivery::reservation_hold(&limits);
        assert!(
            held >= retry * 2 + Duration::from_secs(30),
            "{held:?} for a retry of {retry:?}"
        );
    }
}

/// A terminal event is logged after the store rules on it. A stop between its
/// save and its log write leaves the log without it; the stored position is
/// what says it was stored.
#[tokio::test(start_paused = true)]
async fn a_stop_between_a_terminal_save_and_its_log_write_still_delivers_the_ending() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::After);
    let running = rig.run_on_a(status(1, TaskState::Completed));
    parked(&rig).await;
    rig.kill_a(running).await;
    let id = TaskId::new("t");
    assert!(
        rig.store
            .inner
            .read_events(&id, 0, 10)
            .await
            .unwrap()
            .is_empty(),
        "a terminal event is not in the log yet"
    );

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: Some(TaskState::Completed)
        }
    );
    assert_eq!(rig.rows().await, 0);
}

/// A processor that is only slow (not stopped) is not raced past the hold's
/// end: its reservation was settled as not stored, so when its write does
/// complete it records the deliveries afresh, and the webhook still hears the
/// event once.
#[tokio::test(start_paused = true)]
async fn a_processor_stalled_past_its_hold_records_afresh_and_the_webhook_hears_once() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::Before);
    let running = rig.run_on_a(status(1, TaskState::Working));
    parked(&rig).await;

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(rig.rows().await, 0, "settled as not stored while it waited");

    rig.store.resume.notify_one();
    running.await.unwrap();
    // One of the two replicas' workers delivers the fresh row.
    let first = tokio::time::timeout(Duration::from_mins(15), async {
        tokio::select! {
            seen = rig.a_seen.recv() => seen.unwrap(),
            seen = rig.b_seen.recv() => seen.unwrap(),
        }
    })
    .await
    .expect("no delivery within 15 minutes of (paused) time");
    assert_eq!(first.state, Some(TaskState::Working));
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err() && rig.b_seen.try_recv().is_err());
    assert_eq!(rig.rows().await, 0);
}

/// Without a task store to settle from, nothing is reserved: the lane records
/// after the store, as before, and one delivery results.
#[tokio::test(start_paused = true)]
async fn without_a_task_store_the_lane_records_after_the_event_is_stored() {
    let rig = Rig::new(TaskState::Working).await;
    let configs = Arc::new(InMemoryPushConfigStore::new());
    configs
        .set(TaskPushNotificationConfig::new("t", "https://example.com/hook").with_id("cfg"))
        .await
        .unwrap();
    let (seen, mut rx) = mpsc::unbounded_channel();
    let lanes = crate::handler::PushLanes::with_outbox(
        configs,
        Some(Arc::new(Sender {
            replica: "plain",
            seen,
        }) as Arc<dyn PushSender>),
        rig.limits.clone(),
        Arc::new(crate::metrics::NoopMetrics),
        TaskTracker::new(),
        Some((
            Arc::clone(&rig.outbox) as Arc<dyn PushOutbox>,
            CancellationToken::new(),
        )),
    );
    assert!(
        lanes
            .reserve(
                &TaskId::new("t"),
                Some(1),
                &status(1, TaskState::Working).event
            )
            .await
            .is_none()
    );
    lanes.push(&TaskId::new("t"), &status(1, TaskState::Working).event);
    assert_eq!(
        tokio::time::timeout(Duration::from_mins(15), rx.recv())
            .await
            .expect("no delivery")
            .unwrap()
            .state,
        Some(TaskState::Working)
    );
    rig.wait(Duration::from_secs(30)).await;
    assert!(rx.try_recv().is_err());
}

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
