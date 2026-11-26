// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A store outage longer than `store_write_retry` (COR-01, register X-10):
//! the task's ending is written once the store returns, the events queued
//! behind a write that spent the whole budget are not each given it again,
//! and a shutdown during the outage leaves the ending owed to the reaper.
//!
//! Every case runs on a paused clock, so each duration asserted is the one
//! the retries waited, not a wall-clock reading.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId, TaskState};
use tokio_util::sync::CancellationToken;

use super::tests::{Fixture, artifact, status, task};
use super::{BackgroundDeps, Processor};
use crate::handler::HandlerLimits;
use crate::store::{InMemoryTaskStore, RecordedEvent, TaskStore};

type Fut<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<T>> + Send + 'a>>;

/// The budget each case gives a write.
const RETRY: Duration = Duration::from_secs(1);

/// An in-memory store that refuses every write as unavailable while `down`.
struct Outage {
    inner: InMemoryTaskStore,
    down: AtomicBool,
    /// Refuse with an answer (an internal error) rather than as unavailable.
    answers: AtomicBool,
    /// Writes refused so far.
    refused: AtomicUsize,
}

impl Outage {
    async fn new() -> Self {
        let inner = InMemoryTaskStore::new();
        inner.save(&task(TaskState::Working)).await.expect("seed");
        Self {
            inner,
            down: AtomicBool::new(false),
            answers: AtomicBool::new(false),
            refused: AtomicUsize::new(0),
        }
    }

    fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    fn refuse<'a, T: Send + 'a>(&'a self) -> Option<Fut<'a, T>> {
        self.down.load(Ordering::SeqCst).then(|| {
            self.refused.fetch_add(1, Ordering::SeqCst);
            let error = if self.answers.load(Ordering::SeqCst) {
                A2aError::internal("the store refuses this write")
            } else {
                A2aError::unavailable("the store is down")
            };
            let refused: Fut<'a, T> = Box::pin(async move { Err(error) });
            refused
        })
    }

    async fn state(&self) -> TaskState {
        self.inner
            .get(&TaskId::new("t"))
            .await
            .expect("read")
            .expect("stored")
            .status
            .state
    }

    async fn logged(&self) -> Vec<u64> {
        self.inner
            .read_events(&TaskId::new("t"), 0, 100)
            .await
            .expect("log")
            .into_iter()
            .map(|e| e.seq)
            .collect()
    }
}

impl TaskStore for Outage {
    fn save<'a>(&'a self, task: &'a Task) -> Fut<'a, ()> {
        self.refuse().unwrap_or_else(|| self.inner.save(task))
    }
    fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
        self.inner.get(id)
    }
    fn list<'a>(&'a self, p: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
        self.inner.list(p)
    }
    fn insert_if_absent<'a>(&'a self, task: &'a Task) -> Fut<'a, bool> {
        self.refuse()
            .unwrap_or_else(|| self.inner.insert_if_absent(task))
    }
    fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
        self.inner.delete(id)
    }
    fn supports_event_log(&self) -> bool {
        true
    }
    fn append_event<'a>(
        &'a self,
        id: &'a TaskId,
        seq: u64,
        event: &'a StreamResponse,
    ) -> Fut<'a, ()> {
        self.refuse()
            .unwrap_or_else(|| self.inner.append_event(id, seq, event))
    }
    fn last_event_seq<'a>(&'a self, id: &'a TaskId) -> Fut<'a, u64> {
        self.inner.last_event_seq(id)
    }
    fn read_events<'a>(
        &'a self,
        id: &'a TaskId,
        after: u64,
        limit: usize,
    ) -> Fut<'a, Vec<RecordedEvent>> {
        self.inner.read_events(id, after, limit)
    }
    fn earliest_event_seq<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<u64>> {
        self.inner.earliest_event_seq(id)
    }
    fn event_log_covers<'a>(&'a self, id: &'a TaskId, after: u64) -> Fut<'a, bool> {
        self.inner.event_log_covers(id, after)
    }
}

/// The limits each case runs under, and the same with no write retried.
fn limits() -> (HandlerLimits, HandlerLimits) {
    let limits = HandlerLimits::default().with_store_write_retry(RETRY);
    let down = limits.clone().with_store_write_retry(Duration::ZERO);
    (limits, down)
}

fn processor<'a>(
    id: &'a TaskId,
    store: &'a Outage,
    f: &'a Fixture,
    limits: &'a HandlerLimits,
    down: &'a HandlerLimits,
    shutdown: &CancellationToken,
) -> Processor<'a> {
    Processor::new(
        id,
        BackgroundDeps {
            task_store: store,
            push: &f.lanes,
            limits,
            metrics: &crate::metrics::NoopMetrics,
        },
        task(TaskState::Working),
        f.cancel.clone(),
        None,
        None,
    )
    .watching(shutdown.clone(), down)
}

/// The defect itself: the executor completes while the store is down for
/// longer than the budget. Before COR-01 the processor reverted the status,
/// the close-out found the task still `working`, and nothing wrote it again.
#[tokio::test(start_paused = true)]
async fn an_ending_written_during_a_long_outage_is_stored_when_the_store_returns() {
    let f = Fixture::new(TaskState::Working).await;
    let store = Outage::new().await;
    let (limits, down) = limits();
    let shutdown = CancellationToken::new();
    let id = TaskId::new("t");
    let mut run = processor(&id, &store, &f, &limits, &down, &shutdown);

    store.set_down(true);
    run.handle(Ok(status(1, TaskState::Completed))).await;
    assert_eq!(store.state().await, TaskState::Working, "the write gave up");

    // The store returns ten budgets later.
    tokio::join!(run.close_out(), async {
        tokio::time::sleep(10 * RETRY).await;
        store.set_down(false);
    });

    assert_eq!(store.state().await, TaskState::Completed);
    assert_eq!(
        store.logged().await,
        vec![1],
        "the ending is logged at its own position"
    );
    assert_eq!(
        f.pushed().await,
        vec![TaskState::Completed],
        "and pushed, once"
    );
}

/// The breaker: ten events queued behind an outage drain in one budget, not
/// ten (before: each spent the budget on its log append, then again on its
/// fold, twenty budgets in all).
#[tokio::test(start_paused = true)]
async fn events_queued_behind_an_outage_drain_in_one_budget() {
    let f = Fixture::new(TaskState::Working).await;
    let store = Outage::new().await;
    let (limits, down) = limits();
    let shutdown = CancellationToken::new();
    let id = TaskId::new("t");
    let mut run = processor(&id, &store, &f, &limits, &down, &shutdown);

    store.set_down(true);
    let started = tokio::time::Instant::now();
    for seq in 1..=10 {
        run.handle(Ok(artifact(seq))).await;
    }
    assert!(
        started.elapsed() <= RETRY,
        "ten events took {:?}, more than one budget",
        started.elapsed()
    );
}

/// The same for events that are not logged before they are folded (no log
/// position: synthesized ones; a terminal event is logged only after its
/// verdict): there the fold's own failure is what opens the breaker.
#[tokio::test(start_paused = true)]
async fn events_with_no_log_position_drain_in_one_budget_too() {
    let f = Fixture::new(TaskState::Working).await;
    let store = Outage::new().await;
    let (limits, down) = limits();
    let shutdown = CancellationToken::new();
    let id = TaskId::new("t");
    let mut run = processor(&id, &store, &f, &limits, &down, &shutdown);

    store.set_down(true);
    let started = tokio::time::Instant::now();
    for seq in 1..=10 {
        run.handle(Ok(crate::streaming::StreamEvent::unpositioned(
            artifact(seq).event,
        )))
        .await;
    }
    assert!(
        started.elapsed() <= RETRY,
        "ten events took {:?}, more than one budget",
        started.elapsed()
    );
}

/// A new artifact is written again while the store is unavailable, as every
/// other write of the fold is; until 2026-11-26 it was written once, and a
/// blip of any length lost it.
#[tokio::test(start_paused = true)]
async fn a_new_artifact_rides_out_a_blip_shorter_than_the_budget() {
    let f = Fixture::new(TaskState::Working).await;
    let store = Outage::new().await;
    let (limits, down) = limits();
    let shutdown = CancellationToken::new();
    let id = TaskId::new("t");
    let mut run = processor(&id, &store, &f, &limits, &down, &shutdown);

    store.set_down(true);
    tokio::join!(
        run.handle(Ok(crate::streaming::StreamEvent::unpositioned(
            artifact(1).event
        ))),
        async {
            tokio::time::sleep(RETRY / 3).await;
            store.set_down(false);
        }
    );
    let stored = store.inner.get(&id).await.expect("read").expect("stored");
    assert_eq!(
        stored.artifacts.map_or(0, |a| a.len()),
        1,
        "the artifact was kept"
    );
}

/// The breaker closes on the first write the store takes: an event after the
/// outage gets the whole budget again, so a blip shorter than it is ridden
/// out rather than losing the event.
#[tokio::test(start_paused = true)]
async fn a_write_that_lands_closes_the_breaker() {
    let f = Fixture::new(TaskState::Working).await;
    let store = Outage::new().await;
    let (limits, down) = limits();
    let shutdown = CancellationToken::new();
    let id = TaskId::new("t");
    let mut run = processor(&id, &store, &f, &limits, &down, &shutdown);

    store.set_down(true);
    run.handle(Ok(artifact(1))).await; // opens the breaker
    store.set_down(false);
    run.handle(Ok(artifact(2))).await; // lands: closes it

    // A blip of a third of the budget during the next event.
    store.set_down(true);
    tokio::join!(run.handle(Ok(artifact(3))), async {
        tokio::time::sleep(RETRY / 3).await;
        store.set_down(false);
    });
    assert_eq!(
        store.logged().await,
        vec![2, 3],
        "event 3 waited the blip out"
    );
}

/// A shutdown during the outage: one more budget, then the ending is left
/// owed, so the turn's claim stays for the reaper instead of being released
/// over a task nobody will finish.
#[tokio::test(start_paused = true)]
async fn a_shutdown_during_the_outage_leaves_the_ending_owed() {
    let f = Fixture::new(TaskState::Working).await;
    let store = Outage::new().await;
    let (limits, down) = limits();
    let shutdown = CancellationToken::new();
    let id = TaskId::new("t");
    let mut run = processor(&id, &store, &f, &limits, &down, &shutdown);

    store.set_down(true);
    run.handle(Ok(status(1, TaskState::Completed))).await;
    shutdown.cancel();
    let started = tokio::time::Instant::now();
    let ((), owed) = crate::handler::event_processing::owing_ending(run.close_out()).await;

    assert!(owed, "the ending is left to the reaper");
    assert!(
        started.elapsed() <= 2 * RETRY,
        "shutdown waited {:?}, more than one further budget and a back-off",
        started.elapsed()
    );
    assert_eq!(store.state().await, TaskState::Working);
    assert!(store.refused.load(Ordering::SeqCst) > 0);
}

/// An ending the store refused, as against one it could not take, is not
/// written again: the store is up and has ruled, so retrying would only spin
/// (the processor's close-out would never return).
#[tokio::test(start_paused = true)]
async fn an_ending_the_store_refused_is_not_written_again() {
    let f = Fixture::new(TaskState::Working).await;
    let store = Outage::new().await;
    let (limits, down) = limits();
    let shutdown = CancellationToken::new();
    let id = TaskId::new("t");
    let mut run = processor(&id, &store, &f, &limits, &down, &shutdown);

    store.answers.store(true, Ordering::SeqCst);
    store.set_down(true);
    run.handle(Ok(status(1, TaskState::Completed))).await;
    let attempts = store.refused.load(Ordering::SeqCst);

    tokio::time::timeout(10 * RETRY, run.close_out())
        .await
        .expect("the close-out returns: nothing is written again");
    assert_eq!(
        store.refused.load(Ordering::SeqCst),
        attempts,
        "no write was tried again"
    );
    assert_eq!(store.state().await, TaskState::Working);
}

/// The budget a shutdown leaves is used: a store that returns inside it takes
/// the ending, so nothing is left owed to the reaper.
#[tokio::test(start_paused = true)]
async fn a_store_that_returns_after_shutdown_began_still_takes_the_ending() {
    let f = Fixture::new(TaskState::Working).await;
    let store = Outage::new().await;
    let (limits, down) = limits();
    let shutdown = CancellationToken::new();
    let id = TaskId::new("t");
    let mut run = processor(&id, &store, &f, &limits, &down, &shutdown);

    store.set_down(true);
    run.handle(Ok(status(1, TaskState::Completed))).await;
    shutdown.cancel();
    let (((), owed), ()) = tokio::join!(
        crate::handler::event_processing::owing_ending(run.close_out()),
        async {
            tokio::time::sleep(RETRY / 2).await;
            store.set_down(false);
        }
    );

    assert!(!owed, "the ending was taken, not left to the reaper");
    assert_eq!(store.state().await, TaskState::Completed);
}
