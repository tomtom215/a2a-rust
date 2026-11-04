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

mod lock;
mod reading;
mod retries;
mod window;
