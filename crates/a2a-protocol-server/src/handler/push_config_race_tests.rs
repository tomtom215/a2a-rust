// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The per-task push-config cap holds across replicas (audit E-F14).
//!
//! The handler serialises a create with a lock it keeps per task, which is a
//! lock in one process. Two replicas sharing a store each read a count under
//! the cap and each store, and nothing re-checks a config once it is stored.
//! A second replica is played by a second handler over the same stores, and the
//! interleaving that loses is forced: both read the count before either stores.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
use tokio::sync::Barrier;

use crate::RequestHandler;
use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::handler::HandlerLimits;
use crate::push::{InMemoryPushConfigStore, PushConfigStore, PushSender};
use crate::store::{InMemoryTaskStore, TaskStore};

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

struct Accepts;
impl PushSender for Accepts {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// A push-config store whose first `parties` calls to `list` wait for each
/// other, so that each of `parties` creates has read the count before any of
/// them stores. Later calls (the recount after a store) pass straight through.
struct Rendezvous {
    inner: InMemoryPushConfigStore,
    barrier: Barrier,
    lists: AtomicUsize,
    parties: usize,
}

impl PushConfigStore for Arc<Rendezvous> {
    fn set<'a>(
        &'a self,
        config: TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> {
        self.inner.set(config)
    }
    fn get<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>
    {
        self.inner.get(task_id, id)
    }
    fn list<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>> {
        Box::pin(async move {
            let before = self.lists.fetch_add(1, Ordering::SeqCst);
            let read = self.inner.list(task_id).await;
            if before < self.parties {
                self.barrier.wait().await;
            }
            read
        })
    }
    fn delete<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.delete(task_id, id)
    }
}

fn replica(
    tasks: &Arc<dyn TaskStore>,
    configs: &Arc<Rendezvous>,
    cap: usize,
) -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_task_store_arc(Arc::clone(tasks))
            .with_push_config_store(Arc::clone(configs))
            .with_push_sender(Accepts)
            .with_handler_limits(HandlerLimits::default().with_max_push_configs_per_task(cap))
            .build()
            .expect("handler"),
    )
}

fn create(n: usize) -> TaskPushNotificationConfig {
    TaskPushNotificationConfig::new("t1".to_owned(), format!("https://example.com/hook-{n}"))
        .with_id(format!("cfg-{n}"))
}

/// Two replicas, a cap of one, both reading an empty task before either
/// stores. Whatever the interleaving after that, the task ends with at most
/// the cap, and a create that answered `Ok` is a config that is stored.
#[tokio::test]
async fn two_replicas_creating_at_once_cannot_exceed_the_per_task_cap() {
    for round in 0..20 {
        let tasks: Arc<dyn TaskStore> = Arc::new(InMemoryTaskStore::new());
        tasks
            .save(&Task::new(
                TaskId::new("t1"),
                ContextId::new("ctx"),
                TaskStatus::new(TaskState::Submitted),
            ))
            .await
            .unwrap();
        let configs = Arc::new(Rendezvous {
            inner: InMemoryPushConfigStore::new(),
            barrier: Barrier::new(2),
            lists: AtomicUsize::new(0),
            parties: 2,
        });
        let (a, b) = (replica(&tasks, &configs, 1), replica(&tasks, &configs, 1));

        let (ra, rb) = tokio::join!(
            a.on_set_push_config(create(1), None),
            b.on_set_push_config(create(2), None),
        );

        let stored = configs.inner.list("t1").await.unwrap();
        assert!(
            stored.len() <= 1,
            "round {round}: {} configs stored under a cap of 1",
            stored.len()
        );
        let admitted = usize::from(ra.is_ok()) + usize::from(rb.is_ok());
        assert_eq!(
            admitted,
            stored.len(),
            "round {round}: a create answered Ok exactly when its config is stored: {ra:?} {rb:?}"
        );
        for refused in [&ra, &rb].into_iter().filter(|r| r.is_err()) {
            assert!(
                matches!(refused, Err(ServerError::InvalidParams(why)) if why.contains("maximum")),
                "{refused:?}"
            );
        }
    }
}

/// An update of a config that exists is never withdrawn, even where the task
/// is already over the cap (configs stored before the cap existed, or by an
/// older release): the cap blocks new configs, not changes to old ones.
#[tokio::test]
async fn updating_a_config_never_withdraws_it_even_when_the_task_is_over_the_cap() {
    let tasks: Arc<dyn TaskStore> = Arc::new(InMemoryTaskStore::new());
    tasks
        .save(&Task::new(
            TaskId::new("t1"),
            ContextId::new("ctx"),
            TaskStatus::new(TaskState::Submitted),
        ))
        .await
        .unwrap();
    let configs = Arc::new(Rendezvous {
        inner: InMemoryPushConfigStore::new(),
        barrier: Barrier::new(1),
        lists: AtomicUsize::new(0),
        parties: 0,
    });
    for n in 1..=3 {
        configs.inner.set(create(n)).await.unwrap();
    }
    let h = replica(&tasks, &configs, 1);

    let updated =
        TaskPushNotificationConfig::new("t1".to_owned(), "https://example.com/moved".to_owned())
            .with_id("cfg-1".to_owned());
    h.on_set_push_config(updated, None)
        .await
        .expect("an update is not a new config");

    let stored = configs.inner.list("t1").await.unwrap();
    assert_eq!(stored.len(), 3, "nothing was withdrawn");
    let moved = configs.inner.get("t1", "cfg-1").await.unwrap().unwrap();
    assert_eq!(moved.url, "https://example.com/moved");

    // A new config on the over-cap task is refused and leaves nothing behind.
    let refused = h.on_set_push_config(create(4), None).await;
    assert!(
        matches!(refused, Err(ServerError::InvalidParams(_))),
        "{refused:?}"
    );
    assert_eq!(configs.inner.list("t1").await.unwrap().len(), 3);
}
