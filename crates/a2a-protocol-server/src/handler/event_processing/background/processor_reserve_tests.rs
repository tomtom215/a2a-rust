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

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{Task, TaskId, TaskState};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::tests::{artifact, status, task};
use super::*;
use crate::handler::limits::HandlerLimits;
use crate::metrics::{Metrics, push_outcome};
use crate::push::outbox::{InMemoryPushOutbox, PushOutbox};
use crate::push::{InMemoryPushConfigStore, PushConfigStore, PushSender};
use crate::store::{InMemoryTaskStore, RecordedEvent, TaskStore};

type F<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

/// Where a write stops.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    Never,
    /// Before the write reaches the store; waits for `resume`.
    Before,
    /// After the store has the write; never returns.
    After,
}

/// An in-memory store whose writes can be made to stop, as a process does.
struct Stopping {
    inner: InMemoryTaskStore,
    mode: Mutex<Stop>,
    /// A write has reached its stop.
    parked: Notify,
    resume: Notify,
}

impl Stopping {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: InMemoryTaskStore::new(),
            mode: Mutex::new(Stop::Never),
            parked: Notify::new(),
            resume: Notify::new(),
        })
    }

    fn stop(&self, at: Stop) {
        *self.mode.lock().unwrap_or_else(PoisonError::into_inner) = at;
    }

    /// Runs `write` with the stop applied: once, then writes go through.
    async fn around<T>(&self, write: F<'_, T>) -> A2aResult<T> {
        let at = std::mem::replace(
            &mut *self.mode.lock().unwrap_or_else(PoisonError::into_inner),
            Stop::Never,
        );
        if at == Stop::Before {
            self.parked.notify_one();
            self.resume.notified().await;
        }
        let done = write.await;
        if at == Stop::After {
            self.parked.notify_one();
            std::future::pending::<()>().await;
        }
        done
    }
}

impl TaskStore for Stopping {
    fn save<'a>(&'a self, t: &'a Task) -> F<'a, ()> {
        Box::pin(self.around(self.inner.save(t)))
    }
    fn save_status_delta<'a>(&'a self, t: &'a Task) -> F<'a, ()> {
        Box::pin(self.around(self.inner.save_status_delta(t)))
    }
    fn save_artifact_delta<'a>(
        &'a self,
        t: &'a Task,
        delta: crate::store::ArtifactDelta,
    ) -> F<'a, ()> {
        Box::pin(self.around(self.inner.save_artifact_delta(t, delta)))
    }
    fn get<'a>(&'a self, id: &'a TaskId) -> F<'a, Option<Task>> {
        self.inner.get(id)
    }
    fn get_with_position<'a>(&'a self, id: &'a TaskId) -> F<'a, Option<(Task, Option<u64>)>> {
        self.inner.get_with_position(id)
    }
    fn list<'a>(
        &'a self,
        p: &'a a2a_protocol_types::params::ListTasksParams,
    ) -> F<'a, a2a_protocol_types::responses::TaskListResponse> {
        self.inner.list(p)
    }
    fn insert_if_absent<'a>(&'a self, t: &'a Task) -> F<'a, bool> {
        self.inner.insert_if_absent(t)
    }
    fn delete<'a>(&'a self, id: &'a TaskId) -> F<'a, ()> {
        self.inner.delete(id)
    }
    fn supports_event_log(&self) -> bool {
        true
    }
    fn append_event<'a>(&'a self, id: &'a TaskId, seq: u64, e: &'a StreamResponse) -> F<'a, ()> {
        self.inner.append_event(id, seq, e)
    }
    fn last_event_seq<'a>(&'a self, id: &'a TaskId) -> F<'a, u64> {
        self.inner.last_event_seq(id)
    }
    fn read_events<'a>(
        &'a self,
        id: &'a TaskId,
        after: u64,
        limit: usize,
    ) -> F<'a, Vec<RecordedEvent>> {
        self.inner.read_events(id, after, limit)
    }
}

/// One delivery as a sender saw it: its event, and which replica's sender.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    replica: &'static str,
    /// The state of a status update; `None` for an artifact update.
    state: Option<TaskState>,
}

struct Sender {
    replica: &'static str,
    seen: mpsc::UnboundedSender<Seen>,
}

impl PushSender for Sender {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> F<'a, ()> {
        let state = match event {
            StreamResponse::StatusUpdate(u) => Some(u.status.state),
            _ => None,
        };
        let _ = self.seen.send(Seen {
            replica: self.replica,
            state,
        });
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct Outcomes(Mutex<Vec<String>>);

impl Metrics for Outcomes {
    fn on_push_delivery(&self, outcome: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(outcome.to_owned());
    }
}

impl Outcomes {
    fn count(&self, outcome: &str) -> usize {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|o| *o == outcome)
            .count()
    }
}

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
    _b: crate::handler::PushLanes,
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
            _b: b,
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
    tokio::time::timeout(Duration::from_secs(900), rx.recv())
        .await
        .expect("no delivery within 15 minutes of (paused) time")
        .expect("the sender is gone")
}

/// The write that is to stop has reached its stop.
async fn parked(rig: &Rig) {
    tokio::time::timeout(Duration::from_secs(900), rig.store.parked.notified())
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
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "armed when the event was stored ({:?}), not settled after the hold ({:?})",
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
    let first = tokio::time::timeout(Duration::from_secs(900), async {
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
        tokio::time::timeout(Duration::from_secs(900), rx.recv())
            .await
            .expect("no delivery")
            .unwrap()
            .state,
        Some(TaskState::Working)
    );
    rig.wait(Duration::from_secs(30)).await;
    assert!(rx.try_recv().is_err());
}
