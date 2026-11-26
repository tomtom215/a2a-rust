// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One look of the reaper over a parked task whose lease lapsed (COR-08),
//! against in-memory stores: what it ends, what it leaves, what it releases,
//! and what an idle look costs the lease store.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_types::cancel_cause::{CancelCause, cause_of};
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use super::super::TurnClaims;
use super::Reaper;
use crate::executor::AgentExecutor;
use crate::request_context::RequestContext;
use crate::store::claim::{InMemoryTurnClaims, TurnClaimStore};
use crate::store::lease::{LapsedLeases, LeaseKeys, now_unix_ms};
use crate::store::{InMemoryLeaseStore, InMemoryTaskStore, LeaseStore, TaskStore};
use crate::streaming::EventQueueWriter;

/// What the executor's `cancel` hook does.
#[derive(Clone, Copy)]
enum Hook {
    Returns,
    Fails,
    Hangs,
}

/// Counts the `cancel` hooks it is asked to run.
struct Executor {
    hook: Hook,
    cancels: AtomicUsize,
}

impl AgentExecutor for Executor {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn cancel<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.cancels.fetch_add(1, Ordering::SeqCst);
        match self.hook {
            Hook::Returns => Box::pin(async { Ok(()) }),
            Hook::Fails => Box::pin(async { Err(A2aError::internal("will not")) }),
            Hook::Hangs => Box::pin(std::future::pending()),
        }
    }
}

/// Counts the calls an idle look makes, by method.
#[derive(Default)]
struct Counting {
    inner: InMemoryLeaseStore,
    lapsed_leases: AtomicUsize,
    sweeps: AtomicUsize,
    other: AtomicUsize,
}

type Fut<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

impl LeaseStore for Counting {
    fn grant<'a>(&'a self, t: &'a str, k: &'a str, ttl: i64, d: i64) -> Fut<'a, ()> {
        self.other.fetch_add(1, Ordering::SeqCst);
        self.inner.grant(t, k, ttl, d)
    }
    fn renew<'a>(&'a self, t: &'a str, k: &'a str, now: i64) -> Fut<'a, bool> {
        self.other.fetch_add(1, Ordering::SeqCst);
        self.inner.renew(t, k, now)
    }
    fn deadline<'a>(&'a self, t: &'a str, k: &'a str) -> Fut<'a, Option<i64>> {
        self.other.fetch_add(1, Ordering::SeqCst);
        self.inner.deadline(t, k)
    }
    fn lapsed_many<'a>(&'a self, leases: &'a [(String, String)]) -> Fut<'a, LapsedLeases> {
        self.other.fetch_add(1, Ordering::SeqCst);
        self.inner.lapsed_many(leases)
    }
    fn release<'a>(&'a self, t: &'a str, k: &'a str) -> Fut<'a, ()> {
        self.other.fetch_add(1, Ordering::SeqCst);
        self.inner.release(t, k)
    }
    fn sweep(&self, now: i64, margin: i64) -> Fut<'_, u64> {
        self.sweeps.fetch_add(1, Ordering::SeqCst);
        self.inner.sweep(now, margin)
    }
    fn lapsed_leases(&self, now: i64, limit: usize) -> Fut<'_, LeaseKeys> {
        self.lapsed_leases.fetch_add(1, Ordering::SeqCst);
        self.inner.lapsed_leases(now, limit)
    }
}

const TTL: Duration = Duration::from_secs(60);

struct Fixture {
    reaper: Reaper,
    tasks: Arc<InMemoryTaskStore>,
    leases: Arc<Counting>,
    claim_store: Arc<InMemoryTurnClaims>,
    executor: Arc<Executor>,
    _claims: Arc<TurnClaims>,
}

fn fixture(hook: Hook) -> Fixture {
    let tasks = Arc::new(InMemoryTaskStore::new());
    let leases = Arc::new(Counting::default());
    let claim_store = Arc::new(InMemoryTurnClaims::new());
    let claims = Arc::new(TurnClaims::new(
        Arc::clone(&claim_store) as Arc<dyn TurnClaimStore>,
        Some("reaper".into()),
        TTL,
        Duration::from_secs(1),
    ));
    let executor = Arc::new(Executor {
        hook,
        cancels: AtomicUsize::new(0),
    });
    let reaper = Reaper {
        claims: Arc::downgrade(&claims),
        task_store: Arc::clone(&tasks) as Arc<dyn TaskStore>,
        push_config_store: Arc::new(crate::push::InMemoryPushConfigStore::new()),
        push_sender: None,
        push_lanes: crate::handler::PushLanes::default(),
        leases: Some(Arc::clone(&leases) as Arc<dyn LeaseStore>),
        executor: Arc::clone(&executor) as Arc<dyn AgentExecutor>,
        queues: crate::streaming::EventQueueManager::new(),
        limits: crate::handler::limits::HandlerLimits::default(),
        metrics: Arc::new(crate::metrics::NoopMetrics),
    };
    Fixture {
        reaper,
        tasks,
        leases,
        claim_store,
        executor,
        _claims: claims,
    }
}

impl Fixture {
    /// Stores task `id` in `state`, leased until `deadline_ms`.
    async fn task(&self, id: &str, state: TaskState, deadline_ms: i64) {
        self.tasks
            .save(&Task::new(
                TaskId::new(id),
                ContextId::new("c"),
                TaskStatus::new(state),
            ))
            .await
            .unwrap();
        self.leases
            .inner
            .grant("", id, 1_000, deadline_ms)
            .await
            .unwrap();
    }

    async fn state(&self, id: &str) -> TaskState {
        self.tasks
            .get(&TaskId::new(id))
            .await
            .unwrap()
            .unwrap()
            .status
            .state
    }

    async fn lease(&self, id: &str) -> Option<i64> {
        self.leases.inner.deadline("", id).await.unwrap()
    }

    /// The states the task's log holds, in order.
    async fn logged(&self, id: &str) -> Vec<TaskState> {
        self.tasks
            .read_events(&TaskId::new(id), 0, 100)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|e| match e.event {
                StreamResponse::StatusUpdate(u) => Some(u.status.state),
                _ => None,
            })
            .collect()
    }

    /// Whether the task's claim is free: a fresh claim on it is granted.
    async fn claim_is_free(&self, id: &str) -> bool {
        self.claim_store
            .claim("", id, "probe", "probe-token", TTL)
            .await
            .unwrap()
    }
}

fn lapsed() -> i64 {
    now_unix_ms() - 1_000
}

fn live() -> i64 {
    now_unix_ms() + 3_600_000
}

/// E03 R8: a parked task whose lease lapsed is canceled with the cause
/// `lease-lapsed`, through the executor's hook, its ending logged, and its
/// lease and claim released. Before COR-08 nothing read its deadline: it
/// stayed `input-required`.
#[tokio::test]
async fn a_parked_task_whose_lease_lapsed_is_canceled() {
    let f = fixture(Hook::Returns);
    f.task("asks", TaskState::InputRequired, lapsed()).await;
    f.task("signs", TaskState::AuthRequired, lapsed()).await;
    f.reaper.look().await.unwrap();
    for id in ["asks", "signs"] {
        let task = f.tasks.get(&TaskId::new(id)).await.unwrap().unwrap();
        assert_eq!(task.status.state, TaskState::Canceled, "{id}");
        assert_eq!(
            task.status.message.as_ref().and_then(cause_of),
            Some(CancelCause::LeaseLapsed),
            "{id}"
        );
        assert_eq!(f.logged(id).await, [TaskState::Canceled], "{id}");
        assert_eq!(f.lease(id).await, None, "{id}: the lease is released");
        assert!(f.claim_is_free(id).await, "{id}: the claim is released");
    }
    assert_eq!(f.executor.cancels.load(Ordering::SeqCst), 2);
}

/// A live lease, a claim another replica holds (a turn runs the task or a
/// send continues it), and a running task are each left as they are.
#[tokio::test]
async fn what_a_live_lease_a_held_claim_or_a_running_turn_protects_is_left() {
    let f = fixture(Hook::Returns);
    f.task("renewed", TaskState::InputRequired, live()).await;
    f.task("continued", TaskState::InputRequired, lapsed())
        .await;
    f.task("running", TaskState::Working, lapsed()).await;
    assert!(
        f.claim_store
            .claim("", "continued", "replica-b", "token-b", TTL)
            .await
            .unwrap()
    );
    f.reaper.look().await.unwrap();
    assert_eq!(f.state("renewed").await, TaskState::InputRequired);
    assert_eq!(f.state("continued").await, TaskState::InputRequired);
    assert_eq!(f.state("running").await, TaskState::Working);
    for id in ["renewed", "continued", "running"] {
        assert!(f.lease(id).await.is_some(), "{id}: the lease is kept");
        assert!(f.logged(id).await.is_empty(), "{id}");
    }
    assert!(
        f.claim_is_free("running").await,
        "the reaper's claim is released"
    );
    assert_eq!(f.executor.cancels.load(Ordering::SeqCst), 0);
}

/// A lapsed lease of a finished task, or of one that no longer exists, is
/// left for the sweep, which keeps it for its margin; the task is not
/// written.
#[tokio::test]
async fn the_lapsed_lease_of_a_finished_or_missing_task_is_left_for_the_sweep() {
    let f = fixture(Hook::Returns);
    f.task("done", TaskState::Completed, lapsed()).await;
    f.leases
        .inner
        .grant("", "gone", 1_000, lapsed())
        .await
        .unwrap();
    f.reaper.look().await.unwrap();
    assert_eq!(f.state("done").await, TaskState::Completed);
    assert_eq!(f.logged("done").await, []);
    assert!(f.lease("done").await.is_some());
    assert!(f.lease("gone").await.is_some());
    assert_eq!(f.executor.cancels.load(Ordering::SeqCst), 0);
}

/// A hook that fails, or never returns, does not keep the task parked: the
/// lapse is not a request the executor may refuse, as for a running turn.
/// The hook that hangs is given up at the claim's time to live.
#[tokio::test(start_paused = true)]
async fn a_hook_that_fails_or_hangs_still_ends_the_task() {
    for hook in [Hook::Fails, Hook::Hangs] {
        let f = fixture(hook);
        f.task("t", TaskState::InputRequired, lapsed()).await;
        let began = tokio::time::Instant::now();
        f.reaper.look().await.unwrap();
        assert_eq!(f.state("t").await, TaskState::Canceled);
        assert_eq!(f.executor.cancels.load(Ordering::SeqCst), 1);
        let waited = began.elapsed();
        match hook {
            Hook::Hangs => assert_eq!(waited, TTL),
            _ => assert_eq!(waited, Duration::ZERO),
        }
    }
}

/// What an idle look asks the lease store: the sweep, which it asked before
/// COR-08, and one list of lapsed leases. Each is one statement on the SQL
/// stores, so an idle replica makes one statement more per look than
/// before (two, not one).
#[tokio::test]
async fn an_idle_look_asks_the_lease_store_twice() {
    let f = fixture(Hook::Returns);
    f.reaper.look().await.unwrap();
    assert_eq!(f.leases.sweeps.load(Ordering::SeqCst), 1);
    assert_eq!(f.leases.lapsed_leases.load(Ordering::SeqCst), 1);
    assert_eq!(f.leases.other.load(Ordering::SeqCst), 0);
}
