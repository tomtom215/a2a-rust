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
    /// After the store has the write; returns when `resume` is notified.
    AfterThenWait,
}

/// A write fault: which task writes fail, and whether the write is applied
/// before it fails (a lost acknowledgement) or not at all (a rejection).
pub(in crate::handler::event_processing) struct Fault {
    when: Arc<dyn Fn(&Task) -> bool + Send + Sync>,
    /// The write reaches the store and then reports an error.
    ack_lost: bool,
    /// How many matching writes still fail; `None` for all of them.
    left: Option<usize>,
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
    /// `get` fails too.
    pub(in crate::handler::event_processing) fail_reads: std::sync::atomic::AtomicBool,
    /// `supports_event_log` says no: nothing to settle a reservation from.
    pub(in crate::handler::event_processing) without_log: std::sync::atomic::AtomicBool,
    /// The next `append_event` never reaches the log.
    park_append: std::sync::atomic::AtomicBool,
    /// Task writes that fail, as a store does while it is unavailable.
    fault: Mutex<Option<Fault>>,
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
            fault: Mutex::new(None),
            unreadable: std::sync::atomic::AtomicBool::new(false),
            fail_reads: std::sync::atomic::AtomicBool::new(false),
            without_log: std::sync::atomic::AtomicBool::new(false),
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

    /// Makes every task write for which `when` holds fail (`times` of them, or
    /// all when `None`): rejected before the store has it, or, when `ack_lost`,
    /// after the store has it.
    pub(in crate::handler::event_processing) fn fail_writes(
        &self,
        when: impl Fn(&Task) -> bool + Send + Sync + 'static,
        ack_lost: bool,
        times: Option<usize>,
    ) {
        *self.fault.lock().unwrap_or_else(PoisonError::into_inner) = Some(Fault {
            when: Arc::new(when),
            ack_lost,
            left: times,
        });
    }

    /// Writes go through again.
    pub(in crate::handler::event_processing) fn heal(&self) {
        *self.fault.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Whether this write of `task` is to fail, and if so whether it is applied first.
    fn faulted(&self, task: &Task) -> Option<bool> {
        let mut fault = self.fault.lock().unwrap_or_else(PoisonError::into_inner);
        let active = fault.as_mut().filter(|f| (f.when)(task))?;
        if let Some(left) = &mut active.left {
            if *left == 0 {
                return None;
            }
            *left -= 1;
        }
        let ack_lost = active.ack_lost;
        drop(fault);
        Some(ack_lost)
    }

    /// A write of `task`, failed as the fault says.
    async fn faulting(&self, task: &Task, write: F<'_, ()>) -> A2aResult<()> {
        match self.faulted(task) {
            None => self.around(write).await,
            Some(false) => Err(a2a_protocol_types::error::A2aError::internal(
                "the task store rejected the write",
            )),
            Some(true) => {
                self.around(write).await?;
                Err(a2a_protocol_types::error::A2aError::internal(
                    "the task store's acknowledgement was lost",
                ))
            }
        }
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
        if at == Stop::AfterThenWait {
            self.parked.notify_one();
            self.resume.notified().await;
        }
        done
    }
}

impl TaskStore for Stopping {
    fn save<'a>(&'a self, t: &'a Task) -> F<'a, ()> {
        Box::pin(self.faulting(t, self.inner.save(t)))
    }
    fn save_status_delta<'a>(&'a self, t: &'a Task) -> F<'a, ()> {
        Box::pin(self.faulting(t, self.inner.save_status_delta(t)))
    }
    fn save_artifact_delta<'a>(
        &'a self,
        t: &'a Task,
        delta: crate::store::ArtifactDelta,
    ) -> F<'a, ()> {
        Box::pin(self.faulting(t, self.inner.save_artifact_delta(t, delta)))
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
        if self.fail_reads.load(std::sync::atomic::Ordering::SeqCst) {
            return Box::pin(async {
                Err(a2a_protocol_types::error::A2aError::internal(
                    "the task store cannot be read",
                ))
            });
        }
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
        !self.without_log.load(std::sync::atomic::Ordering::SeqCst)
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
    replica: &'static str,
    seen: mpsc::UnboundedSender<Seen>,
    /// The `A2A-Delivery-Id` of each delivery that carried one.
    pub(in crate::handler::event_processing) ids: Mutex<Vec<String>>,
}

impl Sender {
    pub(in crate::handler::event_processing) fn new(
        replica: &'static str,
        seen: mpsc::UnboundedSender<Seen>,
    ) -> Self {
        Self {
            replica,
            seen,
            ids: Mutex::new(Vec::new()),
        }
    }
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
        if let Some(id) = crate::push::current_delivery_id() {
            self.ids
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(id);
        }
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

/// An outbox that is the in-memory one, except that reserving can be made to
/// fail, as it does while the outbox database is down.
pub(in crate::handler::event_processing) struct Flaky {
    pub(in crate::handler::event_processing) inner: Arc<crate::push::outbox::InMemoryPushOutbox>,
    pub(in crate::handler::event_processing) fail_reserve: std::sync::atomic::AtomicBool,
    /// `release` fails and removes nothing, as it does while the outbox
    /// database is down.
    pub(in crate::handler::event_processing) fail_release: std::sync::atomic::AtomicBool,
    /// `arm` fails and arms nothing.
    pub(in crate::handler::event_processing) fail_arm: std::sync::atomic::AtomicBool,
    /// `extend_hold` fails: an outbox that cannot renew a hold, or is down.
    pub(in crate::handler::event_processing) fail_extend: std::sync::atomic::AtomicBool,
    /// How many times a worker asked for the reservations to settle.
    pub(in crate::handler::event_processing) held_calls: std::sync::atomic::AtomicUsize,
}

impl Flaky {
    pub(in crate::handler::event_processing) fn new(
        inner: Arc<crate::push::outbox::InMemoryPushOutbox>,
    ) -> Self {
        Self {
            inner,
            fail_reserve: std::sync::atomic::AtomicBool::new(false),
            fail_release: std::sync::atomic::AtomicBool::new(false),
            fail_arm: std::sync::atomic::AtomicBool::new(false),
            fail_extend: std::sync::atomic::AtomicBool::new(false),
            held_calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

type O<'a, T> = crate::push::outbox::OutboxFuture<'a, T>;

impl crate::push::outbox::PushOutbox for Flaky {
    fn record(&self, d: Vec<crate::push::outbox::NewDelivery>, cap: usize) -> O<'_, usize> {
        self.inner.record(d, cap)
    }
    fn claim<'a>(
        &'a self,
        owner: &'a str,
        max: usize,
        lease: std::time::Duration,
    ) -> O<'a, Vec<crate::push::outbox::ClaimedDelivery>> {
        self.inner.claim(owner, max, lease)
    }
    fn complete(&self, seq: i64) -> O<'_, ()> {
        self.inner.complete(seq)
    }
    fn retry_after<'a>(
        &'a self,
        seq: i64,
        owner: &'a str,
        after: std::time::Duration,
    ) -> O<'a, ()> {
        self.inner.retry_after(seq, owner, after)
    }
    fn pending(&self) -> O<'_, usize> {
        self.inner.pending()
    }
    fn supports_renewal(&self) -> bool {
        true
    }
    fn renew<'a>(&'a self, owner: &'a str, lease: std::time::Duration) -> O<'a, usize> {
        self.inner.renew(owner, lease)
    }
    fn supports_reservations(&self) -> bool {
        true
    }
    fn reserve<'a>(
        &'a self,
        reservation: &'a str,
        log_seq: u64,
        d: Vec<crate::push::outbox::NewDelivery>,
        cap: usize,
        hold: std::time::Duration,
    ) -> O<'a, usize> {
        if self.fail_reserve.load(std::sync::atomic::Ordering::SeqCst) {
            return Box::pin(async {
                Err(a2a_protocol_types::error::A2aError::internal(
                    "the push outbox is down",
                ))
            });
        }
        self.inner.reserve(reservation, log_seq, d, cap, hold)
    }
    fn arm<'a>(&'a self, reservation: &'a str) -> O<'a, usize> {
        if self.fail_arm.load(std::sync::atomic::Ordering::SeqCst) {
            return Box::pin(async {
                Err(a2a_protocol_types::error::A2aError::internal(
                    "the push outbox is down",
                ))
            });
        }
        self.inner.arm(reservation)
    }
    fn release<'a>(&'a self, reservation: &'a str) -> O<'a, ()> {
        if self.fail_release.load(std::sync::atomic::Ordering::SeqCst) {
            return Box::pin(async {
                Err(a2a_protocol_types::error::A2aError::internal(
                    "the push outbox is down",
                ))
            });
        }
        self.inner.release(reservation)
    }
    fn extend_hold<'a>(&'a self, reservation: &'a str, hold: std::time::Duration) -> O<'a, usize> {
        if self.fail_extend.load(std::sync::atomic::Ordering::SeqCst) {
            return Box::pin(async {
                Err(a2a_protocol_types::error::A2aError::unsupported_operation(
                    "this push outbox does not extend the hold of a reservation",
                ))
            });
        }
        self.inner.extend_hold(reservation, hold)
    }
    fn held(&self, limit: usize) -> O<'_, Vec<crate::push::outbox::Held>> {
        self.held_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.held(limit)
    }
    fn held_for_task<'a>(
        &'a self,
        tenant: &'a str,
        caller: Option<&'a str>,
        task_id: &'a str,
    ) -> O<'a, Vec<crate::push::outbox::Held>> {
        self.inner.held_for_task(tenant, caller, task_id)
    }
}
