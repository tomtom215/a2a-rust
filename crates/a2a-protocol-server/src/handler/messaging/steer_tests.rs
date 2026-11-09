// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Steering a running task through its inbox (ADR 0036), on one handler: who
//! is refused, what is delivered, and what a peer that does not ask for it
//! keeps getting.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::steering::{self, Delivery, Receipt};
use a2a_protocol_types::task::{Task, TaskId, TaskState};
use tokio::sync::Notify;

use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::executor::AgentExecutor;
use crate::metrics::Metrics;
use crate::request_context::RequestContext;
use crate::steering::{Inbox, SteeringConfig};
use crate::store::TaskStore;
use crate::streaming::EventQueueWriter;
use crate::{EventEmitter, RequestHandler, SendMessageResult};

pub(super) const GUARD: Duration = Duration::from_secs(20);

/// What the executor of a test does with the turn it is given.
#[derive(Clone, Copy)]
pub(super) enum Mode {
    /// Reads its inbox until it holds this many messages, then completes.
    Read(usize),
    /// Reads this many messages once released, then completes: so what
    /// arrives meanwhile waits in the inbox, and in the store behind it.
    ReadAfterRelease(usize),
    /// Never reads the inbox: works until released, then completes.
    Block,
    /// Works until released, then parks the task at `input-required`.
    Park,
}

/// What the test sees of its executor.
#[derive(Default)]
pub(super) struct Probe {
    /// Notified once the executor is running (its turn is registered).
    pub(super) started: Notify,
    /// Lets a `Block` or `Park` executor go on.
    pub(super) release: Notify,
    /// The text of every message the executor read from its inbox.
    pub(super) read: Mutex<Vec<String>>,
    /// The senders of those messages.
    pub(super) from: Mutex<Vec<Option<String>>>,
    /// The text of the message each turn started with.
    pub(super) turns: Mutex<Vec<String>>,
}

pub(super) struct Agent {
    pub(super) mode: Mode,
    pub(super) probe: Arc<Probe>,
}

impl AgentExecutor for Agent {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            self.probe
                .turns
                .lock()
                .unwrap()
                .push(ctx.message.text().unwrap_or_default().to_owned());
            emit.status(TaskState::Working).await?;
            self.probe.started.notify_one();
            match self.mode {
                Mode::Read(n) => {
                    let inbox = ctx.inbox().expect("steering is on");
                    for _ in 0..n {
                        let Some(steered) = inbox.recv().await else {
                            break;
                        };
                        self.probe
                            .read
                            .lock()
                            .unwrap()
                            .push(steered.message.text().unwrap_or_default().to_owned());
                        self.probe.from.lock().unwrap().push(steered.from);
                    }
                    emit.status(TaskState::Completed).await
                }
                Mode::ReadAfterRelease(n) => {
                    self.probe.release.notified().await;
                    let inbox = ctx.inbox().expect("steering is on");
                    for _ in 0..n {
                        let Some(steered) = inbox.recv().await else {
                            break;
                        };
                        self.probe
                            .read
                            .lock()
                            .unwrap()
                            .push(steered.message.text().unwrap_or_default().to_owned());
                    }
                    emit.status(TaskState::Completed).await
                }
                Mode::Block => {
                    self.probe.release.notified().await;
                    emit.status(TaskState::Completed).await
                }
                Mode::Park => {
                    self.probe.release.notified().await;
                    emit.status(TaskState::InputRequired).await
                }
            }
        })
    }
}

/// What the handler's metrics were told about steering.
#[derive(Default)]
pub(super) struct Recorder(pub(super) Mutex<Vec<String>>);

impl Recorder {
    pub(super) fn outcomes(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
    pub(super) fn count(&self, outcome: &str) -> usize {
        self.outcomes().iter().filter(|o| *o == outcome).count()
    }
}

impl Metrics for Recorder {
    fn on_steering(&self, outcome: &str) {
        self.0.lock().unwrap().push(outcome.to_owned());
    }
}

pub(super) struct Rig {
    pub(super) handler: RequestHandler,
    pub(super) probe: Arc<Probe>,
    pub(super) metrics: Arc<Recorder>,
}

pub(super) fn rig(mode: Mode, steering: Option<SteeringConfig>) -> Rig {
    let probe = Arc::new(Probe::default());
    let metrics = Arc::new(Recorder::default());
    let mut builder = RequestHandlerBuilder::new(Agent {
        mode,
        probe: Arc::clone(&probe),
    })
    .without_agent_card()
    .with_metrics(Arc::clone(&metrics));
    if let Some(config) = steering {
        builder = builder.with_steering(config);
    }
    Rig {
        handler: builder.build().expect("handler"),
        probe,
        metrics,
    }
}

pub(super) fn message(task: Option<&str>, text: &str) -> Message {
    let mut message = Message::new(
        MessageId::new(uuid::Uuid::new_v4().to_string()),
        MessageRole::User,
        vec![Part::text(text)],
    );
    message.task_id = task.map(TaskId::new);
    message
}

pub(super) fn background(message: Message) -> MessageSendParams {
    let mut params = MessageSendParams::new(message);
    params.configuration =
        Some(serde_json::from_value(serde_json::json!({ "returnImmediately": true })).unwrap());
    params
}

pub(super) fn steering_to(task: &str, text: &str) -> MessageSendParams {
    let mut message = message(Some(task), text);
    steering::declare(&mut message);
    MessageSendParams::new(message)
}

/// Sends `params` and returns the task the handler answered with.
pub(super) async fn send(
    h: &RequestHandler,
    params: MessageSendParams,
) -> Result<Task, ServerError> {
    match h.on_send_message(params, false, None).await? {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => Ok(task),
        other => panic!("expected a task, got {other:?}"),
    }
}

/// Starts the agent's task and waits until its executor is running.
pub(super) async fn start(rig: &Rig) -> TaskId {
    let task = send(&rig.handler, background(message(None, "work")))
        .await
        .expect("the task starts");
    tokio::time::timeout(GUARD, rig.probe.started.notified())
        .await
        .expect("the executor started");
    task.id
}

pub(super) async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + GUARD;
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

pub(super) async fn reaches(store: &Arc<dyn TaskStore>, id: &TaskId, want: TaskState) -> Task {
    let deadline = tokio::time::Instant::now() + GUARD;
    loop {
        let task = store.get(id).await.unwrap().unwrap();
        if task.status.state == want {
            return task;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{id} is {:?}, never {want:?}",
            task.status.state
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn in_flight(id: &TaskId) -> String {
    format!(
        "task {id} is already being processed; wait for it to reach input-required or a \
         terminal state before sending again"
    )
}

pub(super) fn refused_as_in_flight(result: &Result<Task, ServerError>, id: &TaskId) {
    match result {
        Err(ServerError::UnsupportedOperation(text)) => assert_eq!(text, &in_flight(id)),
        other => panic!("expected today's refusal, got {other:?}"),
    }
}

// ── What a message to a running task is ─────────────────────────────────────

/// The headline: a message sent while the executor works reaches the executor
/// that reads its inbox, in order, and says who sent it; the response is the
/// task with a receipt, not a new task.
#[tokio::test]
async fn an_executor_that_reads_its_inbox_sees_the_messages_sent_while_it_works() {
    let rig = rig(Mode::Read(2), Some(SteeringConfig::new()));
    let id = start(&rig).await;

    let first = send(&rig.handler, steering_to(&id.0, "use the cheaper index"))
        .await
        .expect("delivered");
    assert_eq!(first.id, id, "the response is the running task");
    assert_eq!(
        Receipt::read(&first).unwrap(),
        Some(Receipt::new(Delivery::Inbox))
    );
    send(&rig.handler, steering_to(&id.0, "and stop at 100 rows"))
        .await
        .expect("delivered");

    let store = Arc::clone(&rig.handler.task_store);
    let done = reaches(&store, &id, TaskState::Completed).await;
    assert_eq!(
        *rig.probe.read.lock().unwrap(),
        ["use the cheaper index", "and stop at 100 rows"]
    );
    assert_eq!(rig.metrics.outcomes(), ["delivered", "delivered"]);
    // Nothing started a second turn: one executor ran, with the first message.
    assert_eq!(*rig.probe.turns.lock().unwrap(), ["work"]);
    // The receipt is the response's, not the stored task's.
    assert_eq!(Receipt::read(&done).unwrap(), None);
}

/// An executor that never reads its inbox is unaffected: the message is
/// accepted, the executor finishes as it would have, and what was left unread
/// is counted.
#[tokio::test]
async fn an_executor_that_never_reads_its_inbox_is_unaffected() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    let id = start(&rig).await;
    send(&rig.handler, steering_to(&id.0, "ignored"))
        .await
        .expect("accepted");

    rig.probe.release.notify_one();
    let store = Arc::clone(&rig.handler.task_store);
    reaches(&store, &id, TaskState::Completed).await;
    assert!(rig.probe.read.lock().unwrap().is_empty());
    wait_for("the unread message is counted", || {
        rig.metrics.count("unread") == 1
    })
    .await;
    assert_eq!(rig.metrics.count("delivered"), 1);
}

/// A server with steering off, or a message that does not name the
/// extension, is answered as it always was, in exactly the same words.
#[tokio::test]
async fn a_peer_without_the_extension_is_refused_exactly_as_before() {
    // Steering off, message declares it: the refusal of today.
    let off = rig(Mode::Block, None);
    let id = start(&off).await;
    let declared = send(&off.handler, steering_to(&id.0, "x")).await;
    refused_as_in_flight(&declared, &id);
    let plain = send(
        &off.handler,
        MessageSendParams::new(message(Some(&id.0), "x")),
    )
    .await;
    refused_as_in_flight(&plain, &id);
    assert_eq!(off.metrics.outcomes(), Vec::<String>::new());

    // Steering on, message does not declare it: the same refusal, and
    // nothing in the inbox.
    let on = rig(Mode::Block, Some(SteeringConfig::new()));
    let id = start(&on).await;
    let undeclared = send(
        &on.handler,
        MessageSendParams::new(message(Some(&id.0), "x")),
    )
    .await;
    refused_as_in_flight(&undeclared, &id);
    // A message that names the extension in its metadata only is not one.
    let mut by_metadata = message(Some(&id.0), "x");
    by_metadata.metadata = Some(serde_json::json!({ steering::STEERING_METADATA_KEY: {} }));
    let by_metadata = send(&on.handler, MessageSendParams::new(by_metadata)).await;
    refused_as_in_flight(&by_metadata, &id);
    assert_eq!(on.metrics.outcomes(), Vec::<String>::new());
    // And on the same server, naming the extension delivers.
    send(&on.handler, steering_to(&id.0, "x"))
        .await
        .expect("a declared message is delivered");
    assert_eq!(on.metrics.outcomes(), ["delivered"]);
}

/// A task parked at `input-required` is continued as the specification says:
/// the message is not steering, whatever it declares.
#[tokio::test]
async fn a_message_to_a_parked_task_is_a_continuation_not_steering() {
    let rig = rig(Mode::Park, Some(SteeringConfig::new()));
    let id = start(&rig).await;
    rig.probe.release.notify_one();
    let store = Arc::clone(&rig.handler.task_store);
    reaches(&store, &id, TaskState::InputRequired).await;

    // `Park` parks again on the continuation; release it first.
    rig.probe.release.notify_one();
    let answer = send(&rig.handler, steering_to(&id.0, "here is the answer"))
        .await
        .expect("the continuation is admitted");
    assert_eq!(answer.id, id);
    assert_eq!(Receipt::read(&answer).unwrap(), None, "no receipt");
    assert_eq!(
        *rig.probe.turns.lock().unwrap(),
        ["work", "here is the answer"],
        "the message started a turn, as a continuation does"
    );
    assert_eq!(rig.metrics.outcomes(), Vec::<String>::new());
}

/// A task that finished is refused as it always was: not steering either.
#[tokio::test]
async fn a_message_to_a_finished_task_is_refused_as_before() {
    let rig = rig(Mode::Read(0), Some(SteeringConfig::new()));
    let id = start(&rig).await;
    let store = Arc::clone(&rig.handler.task_store);
    reaches(&store, &id, TaskState::Completed).await;
    // The running turn leaves its token a moment after the last write.
    wait_for("the turn is gone", || {
        rig.handler
            .cancellation_tokens
            .try_read()
            .is_ok_and(|t| t.is_empty())
    })
    .await;
    match send(&rig.handler, steering_to(&id.0, "too late")).await {
        Err(ServerError::UnsupportedOperation(text)) => {
            assert_eq!(
                text,
                format!(
                    "task {id} is in terminal state 'TASK_STATE_COMPLETED' and cannot accept new messages"
                )
            );
        }
        other => panic!("expected the terminal-state refusal, got {other:?}"),
    }
    assert_eq!(rig.metrics.outcomes(), Vec::<String>::new());
}

#[tokio::test]
async fn a_message_to_a_task_that_does_not_exist_is_not_found() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    match send(&rig.handler, steering_to("no-such-task", "x")).await {
        Err(ServerError::TaskNotFound(id)) => assert_eq!(id.0, "no-such-task"),
        other => panic!("expected TaskNotFound, got {other:?}"),
    }
}

/// A message naming another context than the task's is the mismatch the
/// specification names, answered as for any send, and delivers nothing.
#[tokio::test]
async fn a_message_naming_another_context_is_refused_and_delivers_nothing() {
    let rig = rig(Mode::Block, Some(SteeringConfig::new()));
    let id = start(&rig).await;
    let mut params = steering_to(&id.0, "x");
    params.message.context_id = Some("another-context".into());
    let refused = send(&rig.handler, params).await;
    assert!(
        matches!(refused, Err(ServerError::InvalidParams(_))),
        "{refused:?}"
    );
    assert_eq!(rig.metrics.outcomes(), Vec::<String>::new());
    assert_eq!(inbox_len(&rig, &id).await, 0);
}

/// The inbox of the turn running `id`, for assertions about what it holds.
pub(super) async fn inbox_of(rig: &Rig, id: &TaskId) -> Arc<Inbox> {
    let tokens = rig.handler.cancellation_tokens.read().await;
    Arc::clone(
        tokens
            .get(id)
            .expect("the turn is registered")
            .turn
            .inbox()
            .expect("steering is on"),
    )
}

pub(super) async fn inbox_len(rig: &Rig, id: &TaskId) -> usize {
    inbox_of(rig, id).await.len()
}
