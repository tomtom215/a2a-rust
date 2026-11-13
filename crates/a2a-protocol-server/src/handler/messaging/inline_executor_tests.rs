// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! On a single-worker runtime a blocking send runs its executor in the task
//! that collects its events; with more workers the executor has its own.
//!
//! Both layouts must keep every property the two-task one had: shutdown
//! still sees and waits for the executor, a panic still fails the task, the
//! caller is answered at an interrupted state without waiting for the
//! executor to return, and a client that goes away does not stop the
//! collection (N27). `#[tokio::test]` runs on a current-thread runtime, one
//! worker, so the tests below exercise the shared task unless they say
//! otherwise.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{TaskState, TaskStatus};
use tokio::sync::Notify;

use crate::builder::RequestHandlerBuilder;
use crate::executor::AgentExecutor;
use crate::handler::{RequestHandler, SendMessageResult};
use crate::request_context::RequestContext;
use crate::streaming::EventQueueWriter;

type ExecFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>;

/// Writes `first`, says it has started, waits to be released, then writes
/// `last` if there is one.
struct GatedExec {
    first: TaskState,
    last: Option<TaskState>,
    started: Arc<Notify>,
    release: Arc<Notify>,
}

async fn write_state(ctx: &RequestContext, queue: &dyn EventQueueWriter, state: TaskState) {
    queue
        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
            ctx.task_id.clone(),
            ctx.context_id.clone(),
            TaskStatus::with_timestamp(state),
        )))
        .await
        .expect("queue accepts the event");
}

impl AgentExecutor for GatedExec {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> ExecFuture<'a> {
        Box::pin(async move {
            write_state(ctx, queue, self.first).await;
            self.started.notify_one();
            self.release.notified().await;
            if let Some(last) = self.last {
                write_state(ctx, queue, last).await;
            }
            Ok(())
        })
    }
}

struct PanicExec;

impl AgentExecutor for PanicExec {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> ExecFuture<'a> {
        Box::pin(async { panic!("the executor panics mid-send") })
    }
}

fn params() -> MessageSendParams {
    MessageSendParams::new(Message::new(
        MessageId::new("m-inline"),
        MessageRole::User,
        vec![Part::text("hello")],
    ))
}

fn gated(
    first: TaskState,
    last: Option<TaskState>,
) -> (Arc<RequestHandler>, Arc<Notify>, Arc<Notify>) {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let handler = RequestHandlerBuilder::new(GatedExec {
        first,
        last,
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    })
    .without_agent_card()
    .build()
    .expect("handler builds");
    (Arc::new(handler), started, release)
}

fn state_of(result: SendMessageResult) -> TaskState {
    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => task.status.state,
        _ => panic!("a blocking send with status updates answers with a task"),
    }
}

/// Polls `done` for up to two seconds: the release after an executor
/// returns runs on its task, ordered after the response but not with it.
async fn eventually(handler: &RequestHandler, done: impl Fn(&RequestHandler) -> bool) -> bool {
    for _ in 0..200 {
        if done(handler) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// On one worker a blocking send is one task, on the executors tracker, so
/// shutdown counts it as a running executor and waits for it. Nothing is on
/// the background tracker: the code before 2026-10-03 had the collector
/// there, and fails the second assertion.
#[tokio::test]
async fn a_blocking_send_counts_once_as_a_tracked_executor() {
    let (handler, started, release) = gated(TaskState::Working, Some(TaskState::Completed));
    let send = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move { handler.on_send_message(params(), false, None).await }
    });
    started.notified().await;
    assert_eq!(
        handler.in_flight.executors().len(),
        1,
        "the executor is tracked"
    );
    assert_eq!(
        handler.in_flight.background().len(),
        0,
        "no second task collects beside it"
    );
    release.notify_one();
    let answered = send.await.expect("send task").expect("send succeeds");
    assert_eq!(state_of(answered), TaskState::Completed);
    assert!(eventually(&handler, |h| h.in_flight.executors().is_empty()).await);
}

/// An interrupted state answers the caller at once. The executor is still
/// running, holds its token, and releases it only when it returns.
#[tokio::test]
async fn an_interrupted_send_answers_before_its_executor_returns() {
    let (handler, started, release) = gated(TaskState::InputRequired, None);
    let answered = tokio::time::timeout(
        Duration::from_secs(5),
        handler.on_send_message(params(), false, None),
    )
    .await
    .expect("answered while the executor is still blocked")
    .expect("send succeeds");
    started.notified().await;
    assert_eq!(state_of(answered), TaskState::InputRequired);
    assert_eq!(
        handler.in_flight.executors().len(),
        1,
        "the executor still runs"
    );
    assert_eq!(handler.cancellation_tokens.read().await.len(), 1);

    release.notify_one();
    assert!(
        eventually(&handler, |h| h.in_flight.executors().is_empty()).await,
        "the merged task runs the executor to its end after answering"
    );
    assert!(handler.cancellation_tokens.read().await.is_empty());
}

/// A panic in an executor polled inline fails the task, as a panicked
/// executor task's `JoinError` did, and still releases its token.
#[tokio::test]
async fn a_panicking_inline_executor_fails_the_task_and_releases_it() {
    let handler = RequestHandlerBuilder::new(PanicExec)
        .without_agent_card()
        .build()
        .expect("handler builds");
    let answered = handler
        .on_send_message(params(), false, None)
        .await
        .expect("the send itself succeeds");
    assert_eq!(state_of(answered), TaskState::Failed);
    let mut released = false;
    for _ in 0..200 {
        if handler.cancellation_tokens.read().await.is_empty() {
            released = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        released,
        "CleanupGuard releases the token after an inline panic"
    );
}

/// Regression for N27: a client that goes away drops the request future,
/// and had the collection lived in it, the only thing persisting the
/// executor's events went with it, leaving the task `working` for good.
/// Aborting the request mirrors what hyper does to a request whose
/// connection closed; the collection must still run and store `completed`.
#[tokio::test]
async fn a_dropped_request_still_finishes_its_collection() {
    let (handler, started, release) = gated(TaskState::Working, Some(TaskState::Completed));
    let send = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move { handler.on_send_message(params(), false, None).await }
    });
    started.notified().await;
    send.abort();
    assert!(
        send.await.unwrap_err().is_cancelled(),
        "the request was dropped"
    );
    assert_eq!(
        handler.in_flight.executors().len(),
        1,
        "the collection runs on its own tracked task, not in the request"
    );

    release.notify_one();
    let mut stored = None;
    for _ in 0..200 {
        let listed = handler
            .on_list_tasks(a2a_protocol_types::params::ListTasksParams::default(), None)
            .await
            .expect("list");
        stored = listed.tasks.first().map(|t| t.status.state);
        if stored == Some(TaskState::Completed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        stored,
        Some(TaskState::Completed),
        "the collection stopped with the request"
    );
    assert!(eventually(&handler, |h| h.in_flight.executors().is_empty()).await);
}

/// With more than one worker the executor gets its own task again, so it
/// runs beside the collection: two entries on the executors tracker while
/// the send is in flight, and the send still completes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_two_workers_the_executor_has_its_own_task() {
    let (handler, started, release) = gated(TaskState::Working, Some(TaskState::Completed));
    let send = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move { handler.on_send_message(params(), false, None).await }
    });
    started.notified().await;
    assert_eq!(
        handler.in_flight.executors().len(),
        2,
        "executor and collection are separate tracked tasks"
    );
    release.notify_one();
    let answered = send.await.expect("send task").expect("send succeeds");
    assert_eq!(state_of(answered), TaskState::Completed);
    assert!(eventually(&handler, |h| h.in_flight.executors().is_empty()).await);
}

// ── An inline executor is polled while its events are folded ────────────────

/// The in-memory store, whose saves take `lock`: a resource the executor
/// also holds, as the audit chain lock is held by `AuditedPolicy` while the
/// folding of an event records the event in the same chain.
struct LockedStore {
    inner: crate::store::InMemoryTaskStore,
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl crate::store::TaskStore for LockedStore {
    fn save<'a>(&'a self, task: &'a a2a_protocol_types::task::Task) -> ExecFuture<'a> {
        Box::pin(async move {
            let _held = self.lock.lock().await;
            self.inner.save(task).await
        })
    }
    fn get<'a>(
        &'a self,
        id: &'a a2a_protocol_types::task::TaskId,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = A2aResult<Option<a2a_protocol_types::task::Task>>>
                + Send
                + 'a,
        >,
    > {
        self.inner.get(id)
    }
    fn list<'a>(
        &'a self,
        params: &'a a2a_protocol_types::params::ListTasksParams,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = A2aResult<a2a_protocol_types::responses::TaskListResponse>,
                > + Send
                + 'a,
        >,
    > {
        self.inner.list(params)
    }
    fn insert_if_absent<'a>(
        &'a self,
        task: &'a a2a_protocol_types::task::Task,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.inner.insert_if_absent(task)
    }
    fn delete<'a>(&'a self, id: &'a a2a_protocol_types::task::TaskId) -> ExecFuture<'a> {
        self.inner.delete(id)
    }
}

/// Holds `lock` while it writes `Working` and waits a moment, then lets it
/// go and completes.
struct LockingExec {
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl AgentExecutor for LockingExec {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> ExecFuture<'a> {
        Box::pin(async move {
            {
                let _held = self.lock.lock().await;
                write_state(ctx, queue, TaskState::Working).await;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            write_state(ctx, queue, TaskState::Completed).await;
            Ok(())
        })
    }
}

/// The collecting task folds an event while the executor it runs holds a
/// lock that folding needs. Until 2026-11-13 the executor was not polled
/// while an event was folded, so it never let the lock go and the send hung
/// until the executor timeout (an hour by default): found by the live
/// approval test (`tests/tool_loop_audit_live.rs`), where the lock was the
/// audit chain's, held by `AuditedPolicy` across a store read.
#[tokio::test]
async fn an_executor_holding_what_a_fold_needs_is_still_polled() {
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    let handler = RequestHandlerBuilder::new(LockingExec {
        lock: Arc::clone(&lock),
    })
    .with_task_store(LockedStore {
        inner: crate::store::InMemoryTaskStore::new(),
        lock: Arc::clone(&lock),
    })
    .without_agent_card()
    .build()
    .expect("handler builds");
    let answered = tokio::time::timeout(
        Duration::from_secs(10),
        handler.on_send_message(params(), false, None),
    )
    .await
    .expect("the send deadlocked: its executor was not polled while an event was folded")
    .expect("send succeeds");
    assert_eq!(state_of(answered), TaskState::Completed);
}
