// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The blocking send reserves an event's outbox rows before it stores the
//! event, as the background processor does (ADR 0037, audit H14).
//!
//! Until 2026-11-11 the collector buffered every event of a request and
//! recorded their rows only when the request ended: a replica lost mid-request
//! lost the deliveries of every event it had already stored. The clock is
//! paused; the stopped replica is a task aborted at a chosen point of the
//! store write, as in `background::processor_reserve_tests`.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::artifact::{Artifact, ArtifactId};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
use tokio::sync::mpsc;
use tokio_util::task::TaskTracker;

use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;
use crate::handler::event_processing::background::reserve_testing::{
    Outcomes, Seen, Sender, Stop, Stopping,
};
use crate::handler::{HandlerLimits, RequestHandler};
use crate::metrics::{Metrics, push_outcome};
use crate::push::outbox::{InMemoryPushOutbox, PushOutbox};
use crate::push::{PushConfigStore, PushSender};
use crate::store::TaskStore;
use crate::streaming::EventQueueWriter;
use crate::streaming::event_queue::{
    DEFAULT_MAX_EVENT_SIZE, DEFAULT_QUEUE_CAPACITY, DEFAULT_WRITE_TIMEOUT,
    new_in_memory_queue_with_shared_persistence,
};

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

fn status(state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        TaskStatus::new(state),
    ))
}

fn artifact() -> StreamResponse {
    StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        Artifact::new(ArtifactId::new("a"), vec![Part::text("x")]),
    ))
}

struct Rig {
    store: Arc<Stopping>,
    outbox: Arc<InMemoryPushOutbox>,
    limits: HandlerLimits,
    metrics: Arc<Outcomes>,
    /// The replica that serves the blocking request.
    handler: Option<Arc<RequestHandler>>,
    a_seen: mpsc::UnboundedReceiver<Seen>,
    /// The replica that takes over.
    _b: crate::handler::PushLanes,
    b_seen: mpsc::UnboundedReceiver<Seen>,
}

impl Rig {
    async fn new() -> Self {
        let store = Stopping::new();
        store
            .inner
            .save(&Task::new(
                "t",
                ContextId::new("c"),
                TaskStatus::new(TaskState::Working),
            ))
            .await
            .unwrap();
        let outbox = Arc::new(InMemoryPushOutbox::new());
        let limits = HandlerLimits::default().with_store_write_retry(Duration::from_secs(1));
        let metrics = Arc::new(Outcomes::default());
        let (a_tx, a_seen) = mpsc::unbounded_channel();
        let handler = RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_task_store_arc(Arc::clone(&store) as Arc<dyn TaskStore>)
            .with_push_sender(Sender::new("a", a_tx))
            .with_push_outbox(Arc::clone(&outbox) as Arc<dyn PushOutbox>)
            .with_handler_limits(limits.clone())
            .with_metrics(MetricsOf(Arc::clone(&metrics)))
            .build()
            .unwrap();
        handler
            .push_config_store
            .set(TaskPushNotificationConfig::new("t", "https://example.com/hook").with_id("cfg"))
            .await
            .unwrap();
        let (b_tx, b_seen) = mpsc::unbounded_channel();
        let b = crate::handler::PushLanes::with_reservations(
            Arc::clone(&handler.push_config_store) as Arc<dyn PushConfigStore>,
            Some(Arc::new(Sender::new("b", b_tx)) as Arc<dyn PushSender>),
            limits.clone(),
            Arc::clone(&metrics) as Arc<dyn Metrics>,
            TaskTracker::new(),
            Some((
                Arc::clone(&outbox) as Arc<dyn PushOutbox>,
                tokio_util::sync::CancellationToken::new(),
            )),
            Some(Arc::clone(&store) as Arc<dyn TaskStore>),
        );
        Self {
            store,
            outbox,
            limits,
            metrics,
            handler: Some(Arc::new(handler)),
            a_seen,
            _b: b,
            b_seen,
        }
    }

    /// A blocking request on the handler that is fed `events` and whose
    /// executor is already done, on a task of its own so it can be stopped.
    fn blocking_request(&self, events: Vec<StreamResponse>) -> tokio::task::JoinHandle<()> {
        let handler = Arc::clone(self.handler.as_ref().expect("replica a is running"));
        tokio::spawn(async move {
            let (writer, _broadcast, reader) = new_in_memory_queue_with_shared_persistence(
                DEFAULT_QUEUE_CAPACITY,
                DEFAULT_MAX_EVENT_SIZE,
                DEFAULT_WRITE_TIMEOUT,
            );
            let writer = writer.collected();
            for event in events {
                writer.write(event).await.unwrap();
            }
            drop(writer);
            let executor = tokio::spawn(async {});
            let _ = handler
                .collect_events(reader, TaskId::new("t"), executor)
                .await;
        })
    }

    /// Replica `a` stops: its request task is aborted and its handler dropped.
    async fn kill_a(&mut self, running: tokio::task::JoinHandle<()>) {
        running.abort();
        let _ = running.await;
        self.handler = None;
    }

    async fn rows(&self) -> usize {
        self.outbox.pending().await.unwrap()
    }
}

/// Counts what reaches the handler's metrics (the handler takes ownership of
/// its recorder).
struct MetricsOf(Arc<Outcomes>);

impl Metrics for MetricsOf {
    fn on_push_delivery(&self, outcome: &str) {
        self.0.on_push_delivery(outcome);
    }
}

async fn next(rx: &mut mpsc::UnboundedReceiver<Seen>) -> Seen {
    tokio::time::timeout(Duration::from_mins(15), rx.recv())
        .await
        .expect("no delivery within 15 minutes of (paused) time")
        .expect("the sender is gone")
}

/// Each event of a request is delivered once, in order, from its own
/// reservation: the batch recorded when the request ends holds none of them.
#[tokio::test(start_paused = true)]
async fn each_event_of_a_blocking_request_is_delivered_once() {
    let mut rig = Rig::new().await;
    let started = tokio::time::Instant::now();
    rig.blocking_request(vec![
        status(TaskState::Working),
        status(TaskState::Completed),
    ])
    .await
    .unwrap();

    for state in [TaskState::Working, TaskState::Completed] {
        assert_eq!(
            next(&mut rig.a_seen).await,
            Seen {
                replica: "a",
                state: Some(state)
            }
        );
    }
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "armed as each event was stored"
    );
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(
        rig.a_seen.try_recv().is_err(),
        "once each, not again at the end"
    );
    assert!(rig.b_seen.try_recv().is_err());
    assert_eq!(rig.rows().await, 0);
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 0);
}

/// The replica serving a blocking request stops after the store has an event
/// and before the request ends: another replica delivers that event.
#[tokio::test(start_paused = true)]
async fn a_replica_lost_mid_request_has_its_stored_event_delivered_by_another() {
    let mut rig = Rig::new().await;
    rig.store.stop(Stop::After);
    let running = rig.blocking_request(vec![artifact()]);
    tokio::time::timeout(Duration::from_mins(15), rig.store.parked.notified())
        .await
        .expect("the write never reached its stop");
    rig.kill_a(running).await;
    assert_eq!(rig.rows().await, 1, "the reservation outlived the request");

    tokio::time::sleep(
        crate::handler::event_processing::background::reservation_hold(&rig.limits)
            + Duration::from_secs(30),
    )
    .await;
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: None
        },
        "the artifact update"
    );
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(rig.b_seen.try_recv().is_err(), "once");
    assert!(rig.a_seen.try_recv().is_err());
    assert_eq!(rig.rows().await, 0);
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 1);
}

/// An event is delivered when it is stored, not when the request ends: the
/// request is held at its second write and the first event has already been
/// delivered. (Until the reservation, its rows were recorded at the end, and a
/// replica lost now delivered nothing.)
#[tokio::test(start_paused = true)]
async fn an_event_is_delivered_while_the_request_is_still_running() {
    let mut rig = Rig::new().await;
    rig.store.stop_after(1, Stop::Before);
    let running = rig.blocking_request(vec![status(TaskState::Working), artifact()]);
    tokio::time::timeout(Duration::from_mins(15), rig.store.parked.notified())
        .await
        .expect("the second write never reached its stop");

    let started = tokio::time::Instant::now();
    assert_eq!(
        next(&mut rig.a_seen).await,
        Seen {
            replica: "a",
            state: Some(TaskState::Working)
        },
        "delivered while the request waits on its second write"
    );
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(!running.is_finished(), "the request is still running");

    rig.store.resume.notify_one();
    running.await.unwrap();
    assert_eq!(
        next(&mut rig.a_seen).await,
        Seen {
            replica: "a",
            state: None
        }
    );
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err(), "once each");
    assert_eq!(rig.rows().await, 0);
}

/// A write the store refuses because it lost a race (the task already ended in
/// another state, or its claim moved to another replica) is settled by the
/// fold, which asked for no push: the rows are removed at once. The store is
/// not asked what became of the event, and could not say here, which would
/// leave the rows for the worker instead.
#[tokio::test(start_paused = true)]
async fn a_write_that_lost_a_race_has_its_rows_removed_without_asking_the_store() {
    use crate::store::{ClaimLost, TerminalStateConflict};

    let task_id = TaskId::new("t");
    let errors = [
        TerminalStateConflict::new(task_id.clone(), TaskState::Failed, TaskState::Completed)
            .into_error(),
        ClaimLost::new(task_id).into_error(),
    ];
    for error in errors {
        let rig = Rig::new().await;
        rig.store
            .refuse_writes_with(|t| t.status.state == TaskState::Completed, error.clone());
        rig.store
            .unreadable
            .store(true, std::sync::atomic::Ordering::SeqCst);

        rig.blocking_request(vec![status(TaskState::Completed)])
            .await
            .unwrap();

        assert_eq!(
            rig.store.reads.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the store was asked what became of the event: {error:?}"
        );
        assert_eq!(
            rig.rows().await,
            0,
            "the refused event leaves no row behind"
        );
        assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 0);
    }
}

/// A write the store applied and then reported as failed (a lost
/// acknowledgement) did store the event: the collector asks the store, finds
/// it, and the event is delivered, once. Settled by the fold alone, the rows
/// would have been removed for an event the store holds.
#[tokio::test(start_paused = true)]
async fn a_write_whose_acknowledgement_was_lost_still_delivers_its_event() {
    let mut rig = Rig::new().await;
    rig.store
        .fail_writes(|t| t.status.state == TaskState::Completed, true, None);

    rig.blocking_request(vec![status(TaskState::Completed)])
        .await
        .unwrap();

    let told = next(&mut rig.a_seen).await;
    assert_eq!(told.state, Some(TaskState::Completed));
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(
        rig.a_seen.try_recv().is_err() && rig.b_seen.try_recv().is_err(),
        "once, not again"
    );
    assert_eq!(rig.rows().await, 0);
}
