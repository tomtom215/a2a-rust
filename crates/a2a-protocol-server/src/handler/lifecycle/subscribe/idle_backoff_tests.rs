// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An idle `SubscribeToTask` stream backs off its polling (audit E-F25).
//!
//! It used to read the store every `subscribe_reattach_interval` (250 ms), about
//! eight statements a second per idle subscriber, for as long as the client
//! stayed. Now the wait doubles while nothing changes, up to
//! `subscribe_reattach_max_interval`, and a change returns from the hook so the
//! next call starts at the interval again. Time is paused: every poll time below
//! is on the virtual clock, so the schedule is asserted exactly.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use super::next_wait;
use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;
use crate::handler::HandlerLimits;
use crate::store::{InMemoryTaskStore, TaskStore};

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

type Fut<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

/// A store that counts reads of a task, which is what one poll is.
struct Counting {
    inner: InMemoryTaskStore,
    reads: Arc<AtomicUsize>,
}

impl TaskStore for Counting {
    fn save<'a>(&'a self, task: &'a Task) -> Fut<'a, ()> {
        self.inner.save(task)
    }
    fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get(id)
    }
    fn list<'a>(&'a self, params: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
        self.inner.list(params)
    }
    fn insert_if_absent<'a>(&'a self, task: &'a Task) -> Fut<'a, bool> {
        self.inner.insert_if_absent(task)
    }
    fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
        self.inner.delete(id)
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The number of store reads a hook has made at each of `at` (milliseconds on
/// the virtual clock, ascending), for a parked task, an interval of `first`, a
/// ceiling of `longest` and an operator's idle bound of `idle` if any, and
/// whether the hook had returned by the last of them.
async fn reads_at(first: u64, longest: u64, idle: Option<u64>, at: &[u64]) -> (Vec<usize>, bool) {
    let reads = Arc::new(AtomicUsize::new(0));
    let store = Counting {
        inner: InMemoryTaskStore::new(),
        reads: Arc::clone(&reads),
    };
    let id = TaskId::new("parked");
    store
        .save(&Task::new(
            id.clone(),
            ContextId::new("ctx"),
            TaskStatus::new(TaskState::InputRequired),
        ))
        .await
        .unwrap();
    let mut limits = HandlerLimits::default()
        .with_subscribe_reattach_interval(ms(first))
        .with_subscribe_reattach_max_interval(ms(longest));
    if let Some(idle) = idle {
        limits = limits.with_subscribe_max_idle(ms(idle));
    }
    let handler = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_task_store_arc(Arc::new(store))
        .with_handler_limits(limits)
        .build()
        .unwrap();
    let hook = handler.subscribe_reattach_hook(id);
    let polling = tokio::spawn((hook)(0));

    let mut seen = Vec::new();
    let mut now = 0;
    tokio::task::yield_now().await;
    for &t in at {
        // A sleep, not `advance`: paused time steps to each timer in turn, so the
        // hook wakes at its own deadline. `advance` jumps the clock in one step
        // and the hook would wake at the end of it and time its next wait from
        // there.
        tokio::time::sleep(ms(t - now)).await;
        now = t;
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        seen.push(reads.load(Ordering::SeqCst));
    }
    let finished = polling.is_finished();
    polling.abort();
    (seen, finished)
}

/// 250 ms doubling to a 2 s ceiling polls at 0, 250, 750, 1750, 3750 and 5750 ms:
/// each reading is taken a millisecond either side of a poll.
#[tokio::test(start_paused = true)]
async fn an_idle_stream_doubles_its_wait_up_to_the_ceiling() {
    let (seen, finished) = reads_at(
        250,
        2_000,
        None,
        &[
            0, 249, 251, 749, 751, 1_749, 1_751, 3_749, 3_751, 5_749, 5_751,
        ],
    )
    .await;
    assert_eq!(seen, vec![1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6]);
    assert!(!finished, "no bound, so the stream stays");
}

/// A ceiling below the first wait is no back-off: the wait stays at the first.
#[tokio::test(start_paused = true)]
async fn a_ceiling_below_the_interval_leaves_the_wait_alone() {
    let (seen, _) = reads_at(250, 100, None, &[249, 251, 499, 501, 749, 751, 999, 1_001]).await;
    assert_eq!(seen, vec![1, 2, 2, 3, 3, 4, 4, 5]);
}

/// An operator's idle bound ends the stream at the bound, not at the first poll
/// after it: with 250 ms doubling, the polls fall at 0, 250, 750 and 1750 ms, and
/// a bound of 1,000 ms is met by a fourth poll the sleep was cut short to reach.
#[tokio::test(start_paused = true)]
async fn a_grown_wait_does_not_carry_the_stream_past_the_idle_bound() {
    let (seen, finished) = reads_at(250, 4_000, Some(1_000), &[749, 751, 999]).await;
    assert_eq!(
        seen,
        vec![2, 3, 3],
        "polls at 0, 250 and 750, then the cut one"
    );
    assert!(!finished, "still waiting for the bound at 999 ms");
    let (seen, finished) = reads_at(250, 4_000, Some(1_000), &[1_001]).await;
    assert_eq!(
        seen,
        vec![4],
        "the cut sleep ends at 1,000 ms and polls once more"
    );
    assert!(finished, "and the stream has ended with the bound");
}

#[test]
fn the_next_wait_doubles_to_the_ceiling_and_stays_there() {
    assert_eq!(next_wait(ms(250), ms(4_000)), ms(500));
    assert_eq!(next_wait(ms(1_500), ms(2_000)), ms(2_000), "capped");
    assert_eq!(next_wait(ms(2_000), ms(2_000)), ms(2_000), "at the ceiling");
    assert_eq!(next_wait(Duration::MAX, ms(2_000)), ms(2_000), "saturates");
    assert_eq!(next_wait(Duration::ZERO, ms(2_000)), Duration::ZERO);
}
