// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A `SubscribeToTask` stream wakes when its store says the task's log changed
//! (audit E-F25 (b), ADR 0057), and never polls faster than its interval.
//!
//! Time is paused, so every poll time below is on the virtual clock and the
//! schedule is asserted exactly. The store is an in-memory one with a registry
//! of waiters (`Watching`), which is what the `PostgreSQL` stores keep their
//! `LISTEN` listener's notifications in.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use super::{pace, wait_for_change};
use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;
use crate::handler::HandlerLimits;
use crate::metrics::Metrics;
use crate::store::log_wake::testing::Watching;
use crate::store::log_wake::{LogWakers, key};
use crate::store::{LogWake, TaskStore};

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

/// The causes the stream reported, in order.
#[derive(Default)]
struct Causes(Mutex<Vec<String>>);

impl Causes {
    fn seen(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl Metrics for Causes {
    fn on_subscribe_wake(&self, cause: &str) {
        self.0.lock().unwrap().push(cause.to_owned());
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn parked(id: &str) -> Task {
    Task::new(
        TaskId::new(id),
        ContextId::new("ctx"),
        TaskStatus::new(TaskState::InputRequired),
    )
}

/// What a stream is, from the outside.
struct Stream {
    store: Arc<Watching>,
    causes: Arc<Causes>,
    task: tokio::task::JoinHandle<crate::streaming::Reattached>,
    at: u64,
}

/// A hook started on a parked task `t` of `store`, with the given interval,
/// ceiling and idle bound (milliseconds).
async fn start(store: Watching, first: u64, longest: u64, idle: Option<u64>) -> Stream {
    let id = TaskId::new("t");
    store.save(&parked("t")).await.unwrap();
    let store = Arc::new(store);
    let causes = Arc::new(Causes::default());
    let mut limits = HandlerLimits::default()
        .with_subscribe_reattach_interval(ms(first))
        .with_subscribe_reattach_max_interval(ms(longest));
    if let Some(idle) = idle {
        limits = limits.with_subscribe_max_idle(ms(idle));
    }
    let handler = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_task_store_arc(Arc::clone(&store) as Arc<dyn TaskStore>)
        .with_metrics(Arc::clone(&causes))
        .with_handler_limits(limits)
        .build()
        .unwrap();
    let hook = handler.subscribe_reattach_hook(id);
    let task = tokio::spawn((hook)(0));
    tokio::task::yield_now().await;
    Stream {
        store,
        causes,
        task,
        at: 0,
    }
}

impl Stream {
    /// Moves the virtual clock to `t` and lets the stream run.
    async fn at(&mut self, t: u64) {
        // A sleep, not `advance`: paused time steps to each timer in turn, so
        // the stream wakes at its own deadline (see `idle_backoff_tests`).
        tokio::time::sleep(ms(t - self.at)).await;
        self.at = t;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }

    fn reads(&self) -> usize {
        self.store.reads()
    }

    fn notify(&self) {
        self.store.wakers.notify(&key("", "t").unwrap());
    }
}

// ── The wake ─────────────────────────────────────────────────────────────────

/// An idle stream polls at 0, 250 and 750 ms and would next poll at 1,750; a
/// notification at 1,000 wakes it then, and the cause is counted.
#[tokio::test(start_paused = true)]
async fn a_notification_wakes_an_idle_stream_at_once() {
    let mut s = start(Watching::new(), 250, 4_000, None).await;
    s.at(749).await;
    assert_eq!(s.reads(), 2);
    s.at(751).await;
    assert_eq!(s.reads(), 3, "the third poll is on the timer");
    s.at(999).await;
    assert_eq!(s.reads(), 3, "asleep until 1,750");
    s.notify();
    s.at(1_001).await;
    assert_eq!(
        s.reads(),
        4,
        "woken at 1,000, a quiet interval after the last poll"
    );
    assert_eq!(s.causes.seen(), ["timeout", "timeout", "notification"]);
    s.task.abort();
}

/// A wake does not make the stream read faster than once an interval: a
/// notification ten milliseconds after a poll is read at 250, and a flurry of
/// them are one read.
#[tokio::test(start_paused = true)]
async fn a_wake_never_polls_faster_than_the_interval() {
    let mut s = start(Watching::new(), 250, 4_000, None).await;
    assert_eq!(s.reads(), 1);
    s.at(10).await;
    for _ in 0..5 {
        s.notify();
    }
    s.at(249).await;
    assert_eq!(s.reads(), 1, "held back to the interval");
    s.at(251).await;
    assert_eq!(s.reads(), 2, "read once, at 250");
    s.at(260).await;
    s.notify();
    s.at(499).await;
    assert_eq!(s.reads(), 2);
    s.at(501).await;
    assert_eq!(s.reads(), 3, "the next at 500");
    assert_eq!(s.causes.seen(), ["notification", "notification"]);
    s.task.abort();
}

/// A notification for another task does not wake it, and a resync wakes it
/// whatever it waits on, as its own cause.
#[tokio::test(start_paused = true)]
async fn another_tasks_notification_is_ignored_and_a_resync_is_not() {
    let mut s = start(Watching::new(), 250, 4_000, None).await;
    s.store.wakers.notify(&key("", "other").unwrap());
    s.at(249).await;
    assert_eq!(s.reads(), 1, "not woken by another task");
    s.at(251).await;
    assert_eq!(s.causes.seen(), ["timeout"]);
    s.store.wakers.resync();
    s.at(260).await;
    assert_eq!(s.causes.seen(), ["timeout", "resync"]);
    s.task.abort();
}

/// A store that cannot wake a stream: the stream polls on its timer alone,
/// exactly as before, and reports nothing.
#[tokio::test(start_paused = true)]
async fn a_store_without_a_watch_polls_on_its_timer_and_reports_nothing() {
    let mut store = Watching::new();
    store.mute = true;
    let mut s = start(store, 250, 4_000, None).await;
    s.at(249).await;
    assert_eq!(s.reads(), 1);
    s.at(251).await;
    assert_eq!(s.reads(), 2);
    s.at(749).await;
    assert_eq!(s.reads(), 2);
    s.at(751).await;
    assert_eq!(s.reads(), 3);
    assert_eq!(s.causes.seen(), Vec::<String>::new());
    assert!(s.store.watches() >= 1, "it was asked");
    s.task.abort();
}

/// The watch is made before each read: one per poll, so an append between a
/// read and the wait is not lost. The notification arrives during the read of
/// the first poll, and wakes the wait that follows, which a timer would not
/// have ended for 250 ms.
#[tokio::test(start_paused = true)]
async fn a_change_during_a_read_wakes_the_wait_that_follows() {
    let store = Watching::new();
    *store.notify_during_read.lock().unwrap() = Some(key("", "t").unwrap());
    let mut s = start(store, 250, 4_000, None).await;
    s.at(1).await;
    assert_eq!(
        s.causes.seen(),
        ["notification"],
        "woken at once, not at 250"
    );
    assert_eq!(
        (s.reads(), s.store.watches()),
        (1, 2),
        "a watch before the read, and another made once the wait ended"
    );
    s.task.abort();
}

/// Once the task has finished the hook answers and drops its watch: nothing is
/// left registered.
#[tokio::test(start_paused = true)]
async fn the_watch_is_dropped_when_the_hook_answers() {
    let mut s = start(Watching::new(), 250, 4_000, None).await;
    assert_eq!(s.store.wakers.watched(), 1);
    let mut done = parked("t");
    done.status = TaskStatus::new(TaskState::Completed);
    s.store.save(&done).await.unwrap();
    s.notify();
    s.at(300).await;
    assert!(s.task.is_finished(), "the task is terminal");
    assert_eq!(s.store.wakers.watched(), 0, "the watch ended with the hook");
}

/// The operator's idle bound still ends a woken stream at the bound: held
/// back by the interval, cut at the deadline.
#[tokio::test(start_paused = true)]
async fn the_idle_bound_ends_a_woken_stream_at_the_bound() {
    let mut s = start(Watching::new(), 250, 4_000, Some(300)).await;
    s.at(10).await;
    s.notify();
    s.at(299).await;
    assert!(!s.task.is_finished());
    s.at(301).await;
    assert!(
        s.task.is_finished(),
        "ended at 300, not at the next interval"
    );
}

// ── The two helpers ──────────────────────────────────────────────────────────

/// With a watch: the timer's end is a timeout, a notification is one, and a
/// resync is its own. Without one it is a sleep that reports nothing.
#[tokio::test(start_paused = true)]
async fn waiting_reports_how_it_ended() {
    let wakers = LogWakers::new();
    let causes = Causes::default();

    let watch = wakers.watch(key("", "a").unwrap());
    let started = tokio::time::Instant::now();
    wait_for_change(Some(watch), ms(500), &causes).await;
    assert_eq!(started.elapsed(), ms(500));

    let watch = wakers.watch(key("", "a").unwrap());
    wakers.notify(&key("", "a").unwrap());
    let started = tokio::time::Instant::now();
    wait_for_change(Some(watch), ms(500), &causes).await;
    assert_eq!(started.elapsed(), Duration::ZERO, "woken, not timed out");

    let watch = wakers.watch(key("", "a").unwrap());
    wakers.resync();
    wait_for_change(Some(watch), ms(500), &causes).await;
    assert_eq!(causes.seen(), ["timeout", "notification", "resync"]);

    let started = tokio::time::Instant::now();
    wait_for_change(None, ms(300), &causes).await;
    assert_eq!(started.elapsed(), ms(300));
    assert_eq!(causes.seen().len(), 3, "a plain sleep reports nothing");
}

/// The pacing holds a poll back to the gap since the last, exactly, cuts the
/// hold at the idle deadline, and holds nothing for the first.
#[tokio::test(start_paused = true)]
async fn pacing_holds_a_poll_back_to_the_gap() {
    let last = Mutex::new(None);
    let now = tokio::time::Instant::now();

    pace(&last, ms(250), None).await;
    assert_eq!(now.elapsed(), Duration::ZERO, "the first poll is not held");
    assert!(last.lock().unwrap().is_some(), "and is recorded");

    tokio::time::sleep(ms(100)).await;
    pace(&last, ms(250), None).await;
    assert_eq!(now.elapsed(), ms(250), "held to 250 ms after the last poll");

    tokio::time::sleep(ms(250)).await;
    pace(&last, ms(250), None).await;
    assert_eq!(
        now.elapsed(),
        ms(500),
        "a full gap since the last: not held"
    );

    tokio::time::sleep(ms(10)).await;
    let deadline = tokio::time::Instant::now() + ms(40);
    pace(&last, ms(250), Some(deadline)).await;
    assert_eq!(now.elapsed(), ms(550), "cut at the idle deadline");

    tokio::time::sleep(ms(260)).await;
    let before = tokio::time::Instant::now();
    pace(&last, ms(250), None).await;
    assert_eq!(before.elapsed(), Duration::ZERO, "more than a gap since");
}

/// The wake's cause is the label the metric carries.
#[test]
fn a_wake_is_reported_under_its_label() {
    assert_eq!(LogWake::Notified.cause(), "notification");
    assert_eq!(LogWake::Resynced.cause(), "resync");
    assert_eq!(crate::metrics::subscribe_wake::TIMEOUT, "timeout");
}
