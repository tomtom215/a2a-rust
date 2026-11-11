// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the push-reservation tests of the background processor and of the
//! blocking collector share: a task store whose writes can be made to stop as
//! a process stops, a sender that reports what it delivered, and a metrics
//! recorder (ADR 0037, audit H14).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{Task, TaskId, TaskState};
use tokio::sync::{Notify, mpsc};

use crate::metrics::Metrics;
use crate::push::PushSender;
use crate::store::{InMemoryTaskStore, RecordedEvent, TaskStore};

pub(in crate::handler::event_processing) type F<'a, T> =
    Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

/// Where a write stops.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::handler::event_processing) enum Stop {
    Never,
    /// Before the write reaches the store; waits for `resume`.
    Before,
    /// After the store has the write; never returns.
    After,
}

/// An in-memory store whose writes can be made to stop, as a process does.
pub(in crate::handler::event_processing) struct Stopping {
    pub(in crate::handler::event_processing) inner: InMemoryTaskStore,
    /// The stop, and how many writes go through before it applies.
    mode: Mutex<(Stop, usize)>,
    /// How many times `get_with_position` was called.
    pub(in crate::handler::event_processing) reads: std::sync::atomic::AtomicUsize,
    /// `get_with_position` fails: the task store cannot be read.
    pub(in crate::handler::event_processing) unreadable: std::sync::atomic::AtomicBool,
    /// The next `append_event` never reaches the log.
    park_append: std::sync::atomic::AtomicBool,
    /// A write has reached its stop.
    pub(in crate::handler::event_processing) parked: Notify,
    pub(in crate::handler::event_processing) resume: Notify,
}

impl Stopping {
    pub(in crate::handler::event_processing) fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: InMemoryTaskStore::new(),
            mode: Mutex::new((Stop::Never, 0)),
            park_append: std::sync::atomic::AtomicBool::new(false),
            unreadable: std::sync::atomic::AtomicBool::new(false),
            reads: std::sync::atomic::AtomicUsize::new(0),
            parked: Notify::new(),
            resume: Notify::new(),
        })
    }

    pub(in crate::handler::event_processing) fn stop(&self, at: Stop) {
        self.stop_after(0, at);
    }

    /// The next log append stops before it is written, as a process does.
    pub(in crate::handler::event_processing) fn park_next_append(&self) {
        self.park_append
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Lets `writes` writes through, then stops the next at `at`.
    pub(in crate::handler::event_processing) fn stop_after(&self, writes: usize, at: Stop) {
        *self.mode.lock().unwrap_or_else(PoisonError::into_inner) = (at, writes);
    }

    /// Runs `write` with the stop applied: once, then writes go through.
    async fn around<T>(&self, write: F<'_, T>) -> A2aResult<T> {
        let at = {
            let mut mode = self.mode.lock().unwrap_or_else(PoisonError::into_inner);
            if mode.1 > 0 {
                mode.1 -= 1;
                Stop::Never
            } else {
                std::mem::replace(&mut mode.0, Stop::Never)
            }
        };
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
    fn try_save_status_delta<'a>(&'a self, t: &'a Task) -> F<'a, bool> {
        self.inner.try_save_status_delta(t)
    }
    fn try_save_artifact_delta<'a>(
        &'a self,
        t: &'a Task,
        delta: crate::store::ArtifactDelta,
    ) -> F<'a, bool> {
        self.inner.try_save_artifact_delta(t, delta)
    }
    fn get<'a>(&'a self, id: &'a TaskId) -> F<'a, Option<Task>> {
        self.inner.get(id)
    }
    fn get_with_position<'a>(&'a self, id: &'a TaskId) -> F<'a, Option<(Task, Option<u64>)>> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.unreadable.load(std::sync::atomic::Ordering::SeqCst) {
            return Box::pin(async {
                Err(a2a_protocol_types::error::A2aError::internal(
                    "the task store cannot be read",
                ))
            });
        }
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
        if self
            .park_append
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Box::pin(async move {
                self.parked.notify_one();
                std::future::pending::<()>().await;
                Ok(())
            });
        }
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
pub(in crate::handler::event_processing) struct Seen {
    pub(in crate::handler::event_processing) replica: &'static str,
    /// The state of a status update; `None` for an artifact update.
    pub(in crate::handler::event_processing) state: Option<TaskState>,
}

pub(in crate::handler::event_processing) struct Sender {
    pub(in crate::handler::event_processing) replica: &'static str,
    pub(in crate::handler::event_processing) seen: mpsc::UnboundedSender<Seen>,
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
pub(in crate::handler::event_processing) struct Outcomes(
    pub(in crate::handler::event_processing) Mutex<Vec<String>>,
);

impl Metrics for Outcomes {
    fn on_push_delivery(&self, outcome: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(outcome.to_owned());
    }
}

impl Outcomes {
    pub(in crate::handler::event_processing) fn count(&self, outcome: &str) -> usize {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|o| *o == outcome)
            .count()
    }
}
