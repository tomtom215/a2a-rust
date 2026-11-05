// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The handler's one lease watcher (`watch`, audit H5): how often it reads,
//! what it leaves alone at a turn's end, and that an answer reaches only the
//! turn it was asked for. The clock is paused.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::lease::Lease;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{TaskId, TaskState};
use tokio::sync::{Notify, Semaphore};

use crate::builder::RequestHandlerBuilder;
use crate::executor::AgentExecutor;
use crate::request_context::RequestContext;
use crate::store::lease::LapsedLeases;
use crate::store::{InMemoryLeaseStore, InMemoryTaskStore, LeaseStore, TaskStore};
use crate::streaming::EventQueueWriter;
use crate::{EventEmitter, RequestHandler, SendMessageResult};

/// Works until cancelled; with `park`, the first turn instead waits for it
/// and asks for input, and a continuation works until cancelled.
#[derive(Default)]
struct Worker {
    park: Option<Arc<Notify>>,
}

impl AgentExecutor for Worker {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            let first = ctx.stored_task.is_none();
            if let (Some(park), true) = (&self.park, first) {
                park.notified().await;
                return emit.status(TaskState::InputRequired).await;
            }
            ctx.cancellation_token.cancelled().await;
            Ok(())
        })
    }
}

/// A lease store whose `lapsed_many` counts its calls and, when gated,
/// waits for a permit and then calls every asked lease lapsed; otherwise it
/// calls none lapsed.
#[derive(Clone, Default)]
struct Scripted {
    inner: InMemoryLeaseStore,
    reads: Arc<AtomicUsize>,
    gate: Option<Arc<Semaphore>>,
}

impl LeaseStore for Scripted {
    fn grant<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
        ttl_ms: i64,
        deadline_ms: i64,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.grant(tenant, task, ttl_ms, deadline_ms)
    }
    fn renew<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
        now_ms: i64,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.inner.renew(tenant, task, now_ms)
    }
    fn deadline<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<i64>>> + Send + 'a>> {
        self.inner.deadline(tenant, task)
    }
    fn lapsed_many<'a>(
        &'a self,
        leases: &'a [(String, String)],
    ) -> Pin<Box<dyn Future<Output = A2aResult<LapsedLeases>> + Send + 'a>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let lapsed = match &self.gate {
                Some(gate) => {
                    gate.acquire().await.expect("gate").forget();
                    true
                }
                None => false,
            };
            let mut out = LapsedLeases::new();
            for key in leases {
                if self.inner.deadline(&key.0, &key.1).await?.is_some() {
                    out.insert(key.clone(), lapsed);
                }
            }
            Ok(out)
        })
    }
    fn release<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.release(tenant, task)
    }
}

fn handler(worker: Worker, leases: &Scripted, tasks: &Arc<dyn TaskStore>) -> RequestHandler {
    RequestHandlerBuilder::new(worker)
        .without_agent_card()
        .with_task_store_arc(Arc::clone(tasks))
        .with_lease_store(leases.clone())
        .build()
        .expect("handler")
}

async fn send(h: &RequestHandler, ttl: Duration, task: Option<&str>) -> String {
    let mut message = Message::new(
        MessageId::new(uuid::Uuid::new_v4().to_string()),
        MessageRole::User,
        vec![Part::text("work")],
    );
    message.task_id = task.map(TaskId::new);
    Lease::new(ttl).attach(&mut message);
    let mut params = MessageSendParams::new(message);
    params.configuration =
        Some(serde_json::from_value(serde_json::json!({ "returnImmediately": true })).unwrap());
    match h.on_send_message(params, false, None).await.unwrap() {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    }
}

async fn state(tasks: &Arc<dyn TaskStore>, id: &str) -> TaskState {
    tasks
        .get(&TaskId::new(id))
        .await
        .unwrap()
        .unwrap()
        .status
        .state
}

/// The watcher reads at the shortest interval any watched lease asks for:
/// with a one-second lease (checked every 250 ms) beside a minute-long one
/// (every second), three seconds hold twelve reads.
#[tokio::test(start_paused = true)]
async fn the_shortest_lease_paces_the_reads() {
    let leases = Scripted::default();
    let tasks: Arc<dyn TaskStore> = Arc::new(InMemoryTaskStore::new());
    let h = handler(Worker::default(), &leases, &tasks);
    send(&h, Duration::from_secs(1), None).await;
    send(&h, Duration::from_secs(60), None).await;
    leases.reads.store(0, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(3_001)).await;
    assert_eq!(leases.reads.load(Ordering::SeqCst), 12);
}

/// A lease the store says is gone is no longer the turn's: when the turn
/// ends it does not release the key, which may by then hold a lease granted
/// elsewhere.
#[tokio::test(start_paused = true)]
async fn a_lease_the_store_dropped_is_not_released_at_the_turns_end() {
    let leases = Scripted::default();
    let tasks: Arc<dyn TaskStore> = Arc::new(InMemoryTaskStore::new());
    let h = handler(Worker::default(), &leases, &tasks);
    let id = send(&h, Duration::from_secs(60), None).await;
    leases.release("", &id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1_001)).await;
    leases.grant("", &id, 60_000, i64::MAX).await.unwrap();
    h.cancel_in_flight(Duration::from_secs(5)).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(leases.deadline("", &id).await.unwrap(), Some(i64::MAX));
}

/// An answer reaches only the turn it was asked about: a turn that ended
/// parked while its lease was being read, and the continuation that took the
/// key since, which holds a new grant, is not stopped by the old answer.
#[tokio::test(start_paused = true)]
async fn an_answer_does_not_reach_the_turn_that_took_the_key_since() {
    let gate = Arc::new(Semaphore::new(0));
    let leases = Scripted {
        gate: Some(Arc::clone(&gate)),
        ..Scripted::default()
    };
    let tasks: Arc<dyn TaskStore> = Arc::new(InMemoryTaskStore::new());
    let park = Arc::new(Notify::new());
    let h = handler(
        Worker {
            park: Some(Arc::clone(&park)),
        },
        &leases,
        &tasks,
    );
    let id = send(&h, Duration::from_secs(60), None).await;
    // The first check is asked about the first turn and waits at the gate.
    tokio::time::sleep(Duration::from_millis(1_001)).await;
    assert_eq!(leases.reads.load(Ordering::SeqCst), 1);
    park.notify_one();
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(state(&tasks, &id).await, TaskState::InputRequired);
    send(&h, Duration::from_secs(60), Some(&id)).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(state(&tasks, &id).await, TaskState::Working);
    // The first check answers "lapsed", about the first turn.
    gate.add_permits(1);
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(state(&tasks, &id).await, TaskState::Working);
}
