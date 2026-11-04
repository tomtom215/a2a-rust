// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The turn is claimed before the idempotency key (audit E-F24).
//!
//! A send that has to wait for another replica's turn holds no key while it
//! waits. Before, it claimed the key first and released it once the claim
//! answered "held elsewhere", two or three database round trips later; a
//! retry routed to another replica in that window found the key and was
//! answered "accepted", and the send it repeated could still be refused when
//! its wait ran out, so the message never ran.
//!
//! What a retry of a send that *is running* finds instead is the key's
//! holder, read without claiming it. Those cases are pinned here too: they
//! are what taking the turn first must not break.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::idempotency::IDEMPOTENCY_METADATA_KEY;
use a2a_protocol_types::message::MessageId;
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams};
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
use tokio::sync::Notify;

use super::KEY;
use super::fixtures::{CountingExecutor, keyed_params, task_of};
use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::executor::AgentExecutor;
use crate::handler::{HandlerLimits, RequestHandler};
use crate::request_context::RequestContext;
use crate::store::task_store::{IdempotencyClaim, IdempotencyHolder};
use crate::store::{HeldClaim, InMemoryTaskStore, InMemoryTurnClaims, TaskStore, TurnClaimStore};
use crate::streaming::EventQueueWriter;

type Fut<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

const TASK: &str = "t-parked";

/// The first `claim` call of a [`Gated`] store: says it has arrived, then
/// waits to be let go.
#[derive(Default)]
struct Pause {
    entered: Notify,
    proceed: Notify,
    taken: AtomicBool,
}

/// An in-memory claim store that can say another replica holds every task,
/// and can hold its first `claim` open. Handlers sharing one `inner` contend
/// for its claims as two replicas sharing a database do.
struct Gated {
    inner: Arc<InMemoryTurnClaims>,
    elsewhere: Arc<AtomicBool>,
    attempts: Arc<AtomicUsize>,
    pause: Option<Arc<Pause>>,
    /// When set, the parked task is stored as completed the moment a claim
    /// is granted: it finished between the send's first read and its claim.
    finish_on_grant: Option<Arc<InMemoryTaskStore>>,
    /// When set, `elsewhere` holds only this task, and every other is free.
    only_task: Option<String>,
}

impl Gated {
    fn new(inner: &Arc<InMemoryTurnClaims>) -> Self {
        Self {
            inner: Arc::clone(inner),
            elsewhere: Arc::new(AtomicBool::new(false)),
            attempts: Arc::new(AtomicUsize::new(0)),
            pause: None,
            finish_on_grant: None,
            only_task: None,
        }
    }
}

impl TurnClaimStore for Gated {
    fn claim<'a>(
        &'a self,
        scope: &'a str,
        task_id: &'a str,
        owner: &'a str,
        token: &'a str,
        ttl: Duration,
    ) -> Fut<'a, bool> {
        Box::pin(async move {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            if let Some(pause) = &self.pause
                && !pause.taken.swap(true, Ordering::SeqCst)
            {
                pause.entered.notify_one();
                pause.proceed.notified().await;
            }
            if self.elsewhere.load(Ordering::SeqCst)
                && self.only_task.as_deref().is_none_or(|only| only == task_id)
            {
                return Ok(false);
            }
            let granted = self.inner.claim(scope, task_id, owner, token, ttl).await?;
            if granted && let Some(store) = &self.finish_on_grant {
                let done = Task::new(
                    TaskId::new(TASK),
                    ContextId::new("ctx-1"),
                    TaskStatus::new(TaskState::Completed),
                );
                store.save(&done).await?;
            }
            Ok(granted)
        })
    }
    fn release<'a>(&'a self, token: &'a str) -> Fut<'a, ()> {
        self.inner.release(token)
    }
    fn renew<'a>(
        &'a self,
        owner: &'a str,
        tokens: &'a [String],
        ttl: Duration,
    ) -> Fut<'a, Vec<HeldClaim>> {
        self.inner.renew(owner, tokens, ttl)
    }
    fn request_cancel<'a>(&'a self, scope: &'a str, task_id: &'a str) -> Fut<'a, bool> {
        self.inner.request_cancel(scope, task_id)
    }
    fn sweep(&self, older_than: Duration) -> Fut<'_, u64> {
        self.inner.sweep(older_than)
    }
}

/// Runs until told to stop: a turn another send finds held.
struct Parked(Arc<Notify>);

impl AgentExecutor for Parked {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Fut<'a, ()> {
        Box::pin(async move {
            self.0.notified().await;
            Ok(())
        })
    }
}

/// Runs until told to stop, counting how many times it started.
struct Counted(Arc<Notify>, Arc<AtomicUsize>);
impl AgentExecutor for Counted {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Fut<'a, ()> {
        self.1.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            self.0.notified().await;
            Ok(())
        })
    }
}

/// A store that cannot honour idempotency keys.
struct NoKeys(InMemoryTaskStore);
impl TaskStore for NoKeys {
    fn save<'a>(&'a self, t: &'a Task) -> Fut<'a, ()> {
        self.0.save(t)
    }
    fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
        self.0.get(id)
    }
    fn list<'a>(&'a self, p: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
        self.0.list(p)
    }
    fn insert_if_absent<'a>(&'a self, t: &'a Task) -> Fut<'a, bool> {
        self.0.insert_if_absent(t)
    }
    fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
        self.0.delete(id)
    }
}

/// A store with a task another turn is waiting on, so a send to it is a
/// continuation.
async fn store_with_a_parked_task() -> Arc<InMemoryTaskStore> {
    let store = Arc::new(InMemoryTaskStore::new());
    let task = Task::new(
        TaskId::new(TASK),
        ContextId::new("ctx-1"),
        TaskStatus::new(TaskState::InputRequired),
    );
    store.save(&task).await.expect("the task saves");
    store
}

/// A continuation of the parked task, carrying [`KEY`], returned at once.
fn continuation(message_id: &str) -> MessageSendParams {
    serde_json::from_value(serde_json::json!({
        "message": {
            "messageId": message_id,
            "role": "ROLE_USER",
            "parts": [{"text": "more"}],
            "taskId": TASK,
            "contextId": "ctx-1",
            "metadata": { IDEMPOTENCY_METADATA_KEY: KEY },
        },
        "configuration": {"returnImmediately": true},
    }))
    .expect("the params parse")
}

fn replica(
    store: &Arc<InMemoryTaskStore>,
    claims: Gated,
    executor: impl AgentExecutor,
    drain: Duration,
) -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(executor)
            .without_agent_card()
            .with_task_store_arc(Arc::clone(store) as Arc<dyn TaskStore>)
            .with_turn_claims(claims)
            .with_handler_limits(HandlerLimits::default().with_executor_drain_timeout(drain))
            .build()
            .expect("the handler builds"),
    )
}

async fn holder(store: &InMemoryTaskStore) -> Option<IdempotencyHolder> {
    store.idempotency_holder(KEY).await.expect("the key reads")
}

// -- the window --------------------------------------------------------------

/// The moment the send's first claim attempt is in flight, its key is free:
/// a retry arriving then has nothing to be answered by. Before, the key was
/// claimed first, so it was already held here.
#[tokio::test(start_paused = true)]
async fn a_send_asking_for_its_turn_holds_no_key() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let pause = Arc::new(Pause::default());
    let mut gated = Gated::new(&claims);
    gated.pause = Some(Arc::clone(&pause));
    gated.elsewhere.store(true, Ordering::SeqCst);
    let elsewhere = Arc::clone(&gated.elsewhere);
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let sent = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move {
            handler
                .on_send_message(continuation("m1"), false, None)
                .await
        }
    });
    pause.entered.notified().await;
    assert_eq!(
        holder(&store).await,
        None,
        "the first claim attempt is in flight and the key is not held"
    );

    // Another replica's turn ends, and the send takes the turn and then the key.
    pause.proceed.notify_one();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        holder(&store).await,
        None,
        "still waiting, still no key (the turn is held elsewhere)"
    );
    elsewhere.store(false, Ordering::SeqCst);
    let answered = sent
        .await
        .expect("the send task ends")
        .expect("the send runs once it holds the turn");
    assert_eq!(task_of(&answered).id, TaskId::new(TASK));
    assert_eq!(
        holder(&store).await,
        Some(IdempotencyHolder::new(
            MessageId::new("m1"),
            TaskId::new(TASK)
        )),
        "the key is taken once the turn is held"
    );
}

/// The task finished between the send's first read and its claim: the send is
/// refused, and the key it had just taken is released, so the caller's retry
/// is not answered with a task that was never continued.
#[tokio::test]
async fn a_send_that_finds_its_task_finished_under_the_claim_releases_its_key() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let mut gated = Gated::new(&claims);
    gated.finish_on_grant = Some(Arc::clone(&store));
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let refused = handler
        .on_send_message(continuation("m1"), false, None)
        .await
        .expect_err("the task finished before the send held the turn");
    assert!(
        matches!(&refused, ServerError::UnsupportedOperation(m) if m.contains("terminal state")),
        "{refused:?}"
    );
    assert_eq!(
        holder(&store).await,
        None,
        "the refused send released its key"
    );
    assert_eq!(executions.load(Ordering::SeqCst), 0, "nothing ran");
}

// -- the per-context lock (audit E-F23) ---------------------------------------

/// A send waiting for another replica's turn does not hold the lock of its
/// context: a send to another task of the same context goes on meanwhile.
/// Held across the wait, it kept the sibling waiting for the whole of it.
#[tokio::test(start_paused = true)]
async fn a_send_waiting_for_a_turn_does_not_hold_up_its_context() {
    let store = store_with_a_parked_task().await;
    store
        .save(&Task::new(
            TaskId::new("t-sibling"),
            ContextId::new("ctx-1"),
            TaskStatus::new(TaskState::InputRequired),
        ))
        .await
        .expect("the sibling saves");
    let claims = Arc::new(InMemoryTurnClaims::new());
    let mut gated = Gated::new(&claims);
    gated.elsewhere.store(true, Ordering::SeqCst);
    gated.only_task = Some(TASK.to_owned());
    let elsewhere = Arc::clone(&gated.elsewhere);
    let attempts = Arc::clone(&gated.attempts);
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let waiting = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move {
            handler
                .on_send_message(continuation("m1"), false, None)
                .await
        }
    });
    while attempts.load(Ordering::SeqCst) < 2 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The first send is in its wait. A send to the sibling is not held up
    // behind it.
    let mut sibling = continuation("m2");
    sibling.message.task_id = Some(TaskId::new("t-sibling"));
    sibling.message.metadata = None;
    let sent = tokio::time::timeout(
        Duration::from_secs(1),
        handler.on_send_message(sibling, false, None),
    )
    .await
    .expect("the sibling send waited behind the send that waits for another replica")
    .expect("the sibling send runs");
    assert_eq!(task_of(&sent).id, TaskId::new("t-sibling"));

    // The turn frees, and the waiting send goes on.
    elsewhere.store(false, Ordering::SeqCst);
    let answered = waiting
        .await
        .expect("the send task ends")
        .expect("the waiting send runs once it holds the turn");
    assert_eq!(task_of(&answered).id, TaskId::new(TASK));
    assert_eq!(executions.load(Ordering::SeqCst), 2, "both sends ran");
}

// -- a retry of a send that runs ---------------------------------------------

/// A retry routed to another replica while the first send runs there is
/// answered with that send's task at once, not made to wait for a turn it
/// will not get and refused.
#[tokio::test]
async fn a_retry_on_another_replica_while_the_send_runs_is_replayed() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let release = Arc::new(Notify::new());
    let a = replica(
        &store,
        Gated::new(&claims),
        Parked(Arc::clone(&release)),
        Duration::from_secs(5),
    );
    let ran_on_b = Arc::new(AtomicUsize::new(0));
    let b = replica(
        &store,
        Gated::new(&claims),
        CountingExecutor(Arc::clone(&ran_on_b)),
        Duration::from_secs(5),
    );

    a.on_send_message(continuation("m1"), false, None)
        .await
        .expect("the send starts on A");
    let started = tokio::time::Instant::now();
    let retried = b
        .on_send_message(continuation("m1"), false, None)
        .await
        .expect("the retry is answered, not refused");
    assert_eq!(task_of(&retried).id, TaskId::new(TASK));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "answered at once and not after waiting out the turn: {:?}",
        started.elapsed()
    );
    assert_eq!(ran_on_b.load(Ordering::SeqCst), 0, "the retry ran nothing");
    release.notify_waiters();
}

/// The same, when the first send runs in this very process: its turn is not
/// "another replica's", and a retry of it is still told so rather than
/// refused as a send to a task already being processed.
#[tokio::test]
async fn a_retry_in_this_process_while_the_send_runs_is_replayed() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let release = Arc::new(Notify::new());
    let a = replica(
        &store,
        Gated::new(&claims),
        Parked(Arc::clone(&release)),
        Duration::from_secs(5),
    );

    a.on_send_message(continuation("m1"), false, None)
        .await
        .expect("the send starts");
    let retried = a
        .on_send_message(continuation("m1"), false, None)
        .await
        .expect("the retry is answered, not refused");
    assert_eq!(task_of(&retried).id, TaskId::new(TASK));
    release.notify_waiters();
}

/// A different message presenting the same key while the turn runs here is
/// a reused key, refused as one is when the turn is free; and a different
/// message with no key, finding the turn held, is refused as busy.
#[tokio::test]
async fn another_message_is_not_taken_for_the_retry() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let release = Arc::new(Notify::new());
    let a = replica(
        &store,
        Gated::new(&claims),
        Parked(Arc::clone(&release)),
        Duration::from_secs(5),
    );
    a.on_send_message(continuation("m1"), false, None)
        .await
        .expect("the send starts");

    let reused = a
        .on_send_message(continuation("m2"), false, None)
        .await
        .expect_err("a second message cannot take the first one's key");
    assert!(
        matches!(&reused, ServerError::InvalidParams(m) if m.contains("held by message `m1`")),
        "{reused:?}"
    );
    let mut unkeyed = continuation("m3");
    unkeyed.message.metadata = None;
    let asked = tokio::time::Instant::now();
    let busy = a
        .on_send_message(unkeyed, false, None)
        .await
        .expect_err("the turn is held");
    assert!(
        matches!(&busy, ServerError::UnsupportedOperation(m) if m.contains("already being processed")),
        "{busy:?}"
    );
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "a turn running in this process is refused at once, not after the wait: {:?}",
        asked.elapsed()
    );
    release.notify_waiters();
}

/// The turn is free and the key is held: the first send finished, and this is
/// its retry. The turn is taken first, the key then names the first send, and
/// the retry is answered with its task and runs nothing.
#[tokio::test]
async fn a_retry_of_a_send_that_finished_is_replayed_once_it_holds_the_turn() {
    let store = store_with_a_parked_task().await;
    store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new(TASK))
        .await
        .expect("the first send's key");
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        Gated::new(&Arc::new(InMemoryTurnClaims::new())),
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let retried = handler
        .on_send_message(continuation("m1"), false, None)
        .await
        .expect("the retry is answered");
    assert_eq!(task_of(&retried).id, TaskId::new(TASK));
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "the retry ran nothing"
    );
}

/// A key the send could never claim is refused before the turn is asked for:
/// nothing is claimed for a send that was never going to run.
#[tokio::test]
async fn a_malformed_key_is_refused_before_the_turn_is_asked_for() {
    let store = store_with_a_parked_task().await;
    let gated = Gated::new(&Arc::new(InMemoryTurnClaims::new()));
    let attempts = Arc::clone(&gated.attempts);
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::new(AtomicUsize::new(0))),
        Duration::from_secs(5),
    );
    let mut params = continuation("m1");
    params.message.metadata = Some(serde_json::json!({ IDEMPOTENCY_METADATA_KEY: "short" }));

    let refused = handler
        .on_send_message(params, false, None)
        .await
        .expect_err("the key is malformed");
    assert!(
        matches!(refused, ServerError::InvalidParams(_)),
        "{refused:?}"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        0,
        "no claim was asked for on behalf of a send that was never going to run"
    );
}

/// Two copies of one message arrive together, on two replicas, while the turn
/// is free: one takes the turn and the key and runs, and the other is answered
/// with that task, at its next poll of the key and not after the turn ends.
#[tokio::test]
async fn two_copies_of_one_message_arriving_together_run_it_once() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let release = Arc::new(Notify::new());
    let ran = Arc::new(AtomicUsize::new(0));
    let a = replica(
        &store,
        Gated::new(&claims),
        Counted(Arc::clone(&release), Arc::clone(&ran)),
        Duration::from_secs(5),
    );
    let b = replica(
        &store,
        Gated::new(&claims),
        Counted(Arc::clone(&release), Arc::clone(&ran)),
        Duration::from_secs(5),
    );

    let asked = tokio::time::Instant::now();
    let (first, second) = tokio::join!(
        a.on_send_message(continuation("m1"), false, None),
        b.on_send_message(continuation("m1"), false, None),
    );
    let first = first.expect("one copy is answered");
    let second = second.expect("the other copy is answered");
    assert_eq!(task_of(&first).id, TaskId::new(TASK));
    assert_eq!(task_of(&second).id, TaskId::new(TASK));
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "answered at the next poll of the key, not when the turn ends: {:?}",
        asked.elapsed()
    );
    // The executor starts after the send has answered. Wait for it, then give a
    // second one time to start: there is none.
    while ran.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(ran.load(Ordering::SeqCst), 1, "the message ran once");
    release.notify_waiters();
}

// -- a send that waits while its retry runs ----------------------------------

/// The retry took the turn first, and with it the key: the send that was
/// waiting is answered with the retry's task and runs nothing, well before
/// its own wait would have run out.
#[tokio::test(start_paused = true)]
async fn a_waiting_send_is_answered_when_its_retry_takes_the_key() {
    let store = store_with_a_parked_task().await;
    let claims = Arc::new(InMemoryTurnClaims::new());
    let gated = Gated::new(&claims);
    gated.elsewhere.store(true, Ordering::SeqCst);
    let attempts = Arc::clone(&gated.attempts);
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = replica(
        &store,
        gated,
        CountingExecutor(Arc::clone(&executions)),
        Duration::from_secs(5),
    );

    let started = tokio::time::Instant::now();
    let sent = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move {
            handler
                .on_send_message(continuation("m1"), false, None)
                .await
        }
    });
    while attempts.load(Ordering::SeqCst) < 3 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The retry, on another replica, won the turn and took the key.
    let taken = store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new(TASK))
        .await
        .expect("the key claims");
    assert_eq!(taken, IdempotencyClaim::Claimed);

    let answered = sent
        .await
        .expect("the send task ends")
        .expect("the waiting send is answered with the retry's task");
    assert_eq!(task_of(&answered).id, TaskId::new(TASK));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "answered when the key was taken, not when the wait ran out: {:?}",
        started.elapsed()
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "the waiting send ran nothing"
    );
}

// -- reading the key ---------------------------------------------------------

fn handler_over_memory() -> RequestHandler {
    RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
        .without_agent_card()
        .build()
        .expect("the handler builds")
}

#[tokio::test]
async fn no_key_and_a_free_key_name_no_replay() {
    let handler = handler_over_memory();
    let unkeyed = keyed_params("m1", None).message;
    assert!(
        handler
            .replay_of_held_key(&unkeyed)
            .await
            .expect("reads")
            .is_none(),
        "a message with no key replays nothing"
    );
    let keyed = keyed_params("m1", Some(KEY)).message;
    assert!(
        handler
            .replay_of_held_key(&keyed)
            .await
            .expect("reads")
            .is_none(),
        "a free key replays nothing"
    );
    assert_eq!(
        holder(&InMemoryTaskStore::new()).await,
        None,
        "and reading it did not take it"
    );
}

#[tokio::test]
async fn a_key_this_message_holds_names_the_task_it_holds() {
    let handler = handler_over_memory();
    let task = Task::new(
        TaskId::new("t-held"),
        ContextId::new("ctx"),
        TaskStatus::new(TaskState::Working),
    );
    handler.task_store.save(&task).await.expect("saves");
    handler
        .task_store
        .claim_idempotency_key(KEY, &MessageId::new("m1"), &TaskId::new("t-held"))
        .await
        .expect("claims");

    let replay = handler
        .replay_of_held_key(&keyed_params("m1", Some(KEY)).message)
        .await
        .expect("reads")
        .expect("the key is held by this message");
    assert_eq!(replay.id, TaskId::new("t-held"));

    let reused = handler
        .replay_of_held_key(&keyed_params("m2", Some(KEY)).message)
        .await
        .expect_err("held by another message");
    assert!(
        matches!(&reused, ServerError::InvalidParams(m) if m.contains("held by message `m1`")),
        "{reused:?}"
    );
}

/// A key the send could never claim is refused before anything is read: the
/// same refusals, in the same order, as the claim gives.
#[tokio::test]
async fn a_key_that_could_never_be_claimed_is_refused_before_it_is_read() {
    let handler = handler_over_memory();
    let mut malformed = keyed_params("m1", None).message;
    malformed.metadata = Some(serde_json::json!({ IDEMPOTENCY_METADATA_KEY: "short" }));
    let refused = handler
        .replay_of_held_key(&malformed)
        .await
        .expect_err("malformed");
    assert!(
        matches!(refused, ServerError::InvalidParams(_)),
        "{refused:?}"
    );

    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
        .without_agent_card()
        .with_task_store(NoKeys(InMemoryTaskStore::new()))
        .build()
        .expect("the handler builds");
    let refused = handler
        .replay_of_held_key(&keyed_params("m1", Some(KEY)).message)
        .await
        .expect_err("the store cannot honour keys");
    assert!(
        matches!(refused, ServerError::UnsupportedOperation(_)),
        "{refused:?}"
    );
}

/// A store that cannot answer is reported as the claim reports it: a
/// temporary refusal as `Overloaded`, anything else as the store's error.
#[tokio::test]
async fn a_failed_read_of_the_key_is_reported_as_a_failed_claim_is() {
    struct Failing(InMemoryTaskStore, A2aError);
    impl TaskStore for Failing {
        fn save<'a>(&'a self, t: &'a Task) -> Fut<'a, ()> {
            self.0.save(t)
        }
        fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
            self.0.get(id)
        }
        fn list<'a>(&'a self, p: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
            self.0.list(p)
        }
        fn insert_if_absent<'a>(&'a self, t: &'a Task) -> Fut<'a, bool> {
            self.0.insert_if_absent(t)
        }
        fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
            self.0.delete(id)
        }
        fn supports_idempotency(&self) -> bool {
            true
        }
        fn idempotency_holder<'a>(&'a self, _key: &'a str) -> Fut<'a, Option<IdempotencyHolder>> {
            let error = self.1.clone();
            Box::pin(async move { Err(error) })
        }
    }
    for (error, overloaded) in [
        (A2aError::unavailable("the database is restarting"), true),
        (A2aError::internal("the key index is corrupt"), false),
    ] {
        let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
            .without_agent_card()
            .with_task_store(Failing(InMemoryTaskStore::new(), error))
            .build()
            .expect("the handler builds");
        let refused = handler
            .replay_of_held_key(&keyed_params("m1", Some(KEY)).message)
            .await
            .expect_err("the read fails");
        assert_eq!(
            matches!(refused, ServerError::Overloaded(_)),
            overloaded,
            "{refused:?}"
        );
    }
}

/// A message with no key reads nothing from the store, and one with a key
/// reads it once: the wait asks before every attempt, and a send that carries
/// no key must not cost a database round trip each time.
#[tokio::test]
async fn only_a_message_with_a_key_reads_the_store() {
    struct Reads(InMemoryTaskStore, Arc<AtomicUsize>);
    impl TaskStore for Reads {
        fn save<'a>(&'a self, t: &'a Task) -> Fut<'a, ()> {
            self.0.save(t)
        }
        fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
            self.0.get(id)
        }
        fn list<'a>(&'a self, p: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
            self.0.list(p)
        }
        fn insert_if_absent<'a>(&'a self, t: &'a Task) -> Fut<'a, bool> {
            self.0.insert_if_absent(t)
        }
        fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
            self.0.delete(id)
        }
        fn supports_idempotency(&self) -> bool {
            true
        }
        fn idempotency_holder<'a>(&'a self, key: &'a str) -> Fut<'a, Option<IdempotencyHolder>> {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0.idempotency_holder(key)
        }
    }
    let reads = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::new(AtomicUsize::new(0))))
        .without_agent_card()
        .with_task_store(Reads(InMemoryTaskStore::new(), Arc::clone(&reads)))
        .build()
        .expect("the handler builds");

    let unkeyed = keyed_params("m1", None).message;
    assert!(
        handler
            .replay_of_held_key(&unkeyed)
            .await
            .expect("reads")
            .is_none()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0, "no key, no read");

    let keyed = keyed_params("m1", Some(KEY)).message;
    assert!(
        handler
            .replay_of_held_key(&keyed)
            .await
            .expect("reads")
            .is_none()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1, "a key is read once");
}
