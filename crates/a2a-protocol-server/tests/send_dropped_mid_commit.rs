// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Release audit F9: a send dropped after its task row is saved and before
//! its executor starts leaves nothing behind.
//!
//! A new task's row is saved before its inline push config is registered, so
//! a client that goes away during that registration used to leave the row
//! `Submitted` for ever with no executor, visible to `GetTask` and
//! `ListTasks`, and its released idempotency key let the retry create a
//! second task. Here the push-config store stores the config and then never
//! answers — the worst case, a row and a config both written — and the send
//! is dropped there.
//!
//! The same drop under a tenant also has to release the key in that tenant's
//! partition: the release runs in a spawned task, which does not inherit the
//! request's tenant scope.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use a2a_protocol_server::push::TenantAwareInMemoryPushConfigStore;
use a2a_protocol_server::push::{InMemoryPushConfigStore, PushConfigStore};
use a2a_protocol_server::store::{TaskStore, TenantAwareInMemoryTaskStore, tenant::TenantContext};
use a2a_protocol_server::{
    EventEmitter, RequestHandler, RequestHandlerBuilder, SendMessageResult, agent_executor,
};
use a2a_protocol_types::Message;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::idempotency::set_key;
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams, SendMessageConfiguration};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;
use std::future::Future;
use std::pin::Pin;

struct Completes;
agent_executor!(Completes, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Completed)
        .await
});

/// Stores the config, then — while armed — never answers.
struct Stalling<S> {
    inner: S,
    armed: AtomicBool,
    entered: tokio::sync::Notify,
    /// The task the stalled config was stored for.
    stalled: std::sync::Mutex<Option<String>>,
}

impl<S: PushConfigStore> PushConfigStore for Stalling<S> {
    fn set<'a>(
        &'a self,
        config: TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> {
        Box::pin(async move {
            let stored = self.inner.set(config).await?;
            if self.armed.load(Ordering::SeqCst) {
                *self.stalled.lock().expect("lock") = stored.task_id.clone();
                self.entered.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(stored)
        })
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
        self.inner.list(task_id)
    }
    fn delete<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.delete(task_id, id)
    }
    fn isolates_tenants(&self) -> bool {
        self.inner.isolates_tenants()
    }
}

fn keyed_with_push(tenant: Option<&str>) -> MessageSendParams {
    let mut message = Message::user_text("m-dropped", "do it");
    set_key(&mut message, "dropped-key-0123456789").expect("key");
    let mut params = MessageSendParams::new(message);
    params.tenant = tenant.map(str::to_owned);
    params.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: vec!["text/plain".into()],
        task_push_notification_config: Some(TaskPushNotificationConfig::new(
            "",
            "https://hooks.example/a2a",
        )),
        history_length: None,
        return_immediately: None,
    });
    params
}

async fn task_ids(store: &dyn TaskStore, tenant: &str) -> Vec<String> {
    TenantContext::scope(tenant, store.list(&ListTasksParams::default()))
        .await
        .expect("list")
        .tasks
        .into_iter()
        .map(|t| t.id.0)
        .collect()
}

/// Sends, drops the send once the push config is stored, then retries the
/// same message and returns the task ids stored before and after the retry.
async fn drop_then_retry<S: PushConfigStore>(
    handler: &Arc<RequestHandler>,
    stalling: &Stalling<S>,
    task_store: &dyn TaskStore,
    tenant: Option<&str>,
) -> (Vec<String>, Vec<String>, String) {
    stalling.armed.store(true, Ordering::SeqCst);
    let entered = stalling.entered.notified();
    let send = tokio::spawn({
        let handler = Arc::clone(handler);
        let params = keyed_with_push(tenant);
        async move { handler.on_send_message(params, false, None).await }
    });
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .expect("the send never reached the push config store");
    send.abort();
    let _ = send.await;
    stalling.armed.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let scope = tenant.unwrap_or("");
    let after_drop = task_ids(task_store, scope).await;

    let retried = tokio::time::timeout(
        Duration::from_secs(10),
        handler.on_send_message(keyed_with_push(tenant), false, None),
    )
    .await
    .expect("the retry hung")
    .expect("the retry runs");
    let id = match retried {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    };
    (after_drop, task_ids(task_store, scope).await, id)
}

#[tokio::test]
async fn a_new_task_dropped_while_its_push_config_registers_is_rolled_back() {
    let stalling = Arc::new(Stalling {
        inner: InMemoryPushConfigStore::new(),
        armed: AtomicBool::new(false),
        entered: tokio::sync::Notify::new(),
        stalled: std::sync::Mutex::default(),
    });
    let task_store: Arc<dyn TaskStore> =
        Arc::new(a2a_protocol_server::store::InMemoryTaskStore::new());
    let handler = Arc::new(
        RequestHandlerBuilder::new(Completes)
            .without_agent_card()
            .with_task_store_arc(Arc::clone(&task_store))
            .with_push_config_store_arc(stalling.clone())
            .with_push_sender(a2a_protocol_server::push::HttpPushSender::new())
            .build()
            .expect("handler"),
    );
    let (after_drop, after_retry, retried) =
        drop_then_retry(&handler, &stalling, &*task_store, None).await;
    assert_eq!(
        after_drop,
        Vec::<String>::new(),
        "the dropped send's row was left behind"
    );
    assert_eq!(
        after_retry,
        std::slice::from_ref(&retried),
        "one send, one task"
    );
    let dropped = stalling
        .stalled
        .lock()
        .expect("lock")
        .clone()
        .expect("stalled");
    assert_ne!(dropped, retried);
    assert_eq!(
        stalling.inner.list(&dropped).await.expect("list").len(),
        0,
        "the dropped send's push config was left behind"
    );
    assert_eq!(stalling.inner.list(&retried).await.expect("list").len(), 1);
}

#[tokio::test]
async fn a_dropped_send_under_a_tenant_releases_its_key_in_that_tenant() {
    let stalling = Arc::new(Stalling {
        inner: TenantAwareInMemoryPushConfigStore::new(),
        armed: AtomicBool::new(false),
        entered: tokio::sync::Notify::new(),
        stalled: std::sync::Mutex::default(),
    });
    let task_store: Arc<dyn TaskStore> = Arc::new(TenantAwareInMemoryTaskStore::new());
    let handler = Arc::new(
        RequestHandlerBuilder::new(Completes)
            .without_agent_card()
            .with_task_store_arc(Arc::clone(&task_store))
            .with_push_config_store_arc(stalling.clone())
            .with_push_sender(a2a_protocol_server::push::HttpPushSender::new())
            .build()
            .expect("handler"),
    );
    let (after_drop, after_retry, retried) =
        drop_then_retry(&handler, &stalling, &*task_store, Some("acme")).await;
    assert_eq!(
        after_drop,
        Vec::<String>::new(),
        "the dropped send's row was left behind"
    );
    assert_eq!(after_retry, [retried], "one send, one task, in the tenant");
}

/// A task store whose `delete` waits on a gate.
struct GatedDelete {
    inner: a2a_protocol_server::store::InMemoryTaskStore,
    gate: tokio::sync::Semaphore,
}

impl TaskStore for GatedDelete {
    fn save<'a>(
        &'a self,
        task: &'a a2a_protocol_types::task::Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.save(task)
    }
    fn get<'a>(
        &'a self,
        id: &'a a2a_protocol_types::task::TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<a2a_protocol_types::task::Task>>> + Send + 'a>>
    {
        self.inner.get(id)
    }
    fn list<'a>(
        &'a self,
        params: &'a ListTasksParams,
    ) -> Pin<
        Box<
            dyn Future<Output = A2aResult<a2a_protocol_types::responses::TaskListResponse>>
                + Send
                + 'a,
        >,
    > {
        self.inner.list(params)
    }
    fn insert_if_absent<'a>(
        &'a self,
        task: &'a a2a_protocol_types::task::Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.inner.insert_if_absent(task)
    }
    fn delete<'a>(
        &'a self,
        id: &'a a2a_protocol_types::task::TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let _permit = self.gate.acquire().await.expect("gate");
            self.inner.delete(id).await
        })
    }
}

fn into_context(mut params: MessageSendParams, id: &str) -> MessageSendParams {
    params.message.context_id = Some(a2a_protocol_types::task::ContextId::new("ctx-race"));
    params.message.id = a2a_protocol_types::MessageId::new(id);
    // The GatedDelete store does not support idempotency keys.
    params.message.metadata = None;
    params
}

/// The rollback holds the context's lock until the row is gone: a second
/// send for the same context waits for it, and then starts a task of its
/// own rather than continuing the one being deleted.
#[tokio::test]
async fn a_send_for_the_same_context_waits_for_the_rollback() {
    let stalling = Arc::new(Stalling {
        inner: InMemoryPushConfigStore::new(),
        armed: AtomicBool::new(true),
        entered: tokio::sync::Notify::new(),
        stalled: std::sync::Mutex::default(),
    });
    let tasks = Arc::new(GatedDelete {
        inner: a2a_protocol_server::store::InMemoryTaskStore::new(),
        gate: tokio::sync::Semaphore::new(0),
    });
    let handler = Arc::new(
        RequestHandlerBuilder::new(Completes)
            .without_agent_card()
            .with_task_store_arc(tasks.clone())
            .with_push_config_store_arc(stalling.clone())
            .with_push_sender(a2a_protocol_server::push::HttpPushSender::new())
            .build()
            .expect("handler"),
    );

    let entered = stalling.entered.notified();
    let first = tokio::spawn({
        let handler = Arc::clone(&handler);
        let params = into_context(keyed_with_push(None), "m-first");
        async move { handler.on_send_message(params, false, None).await }
    });
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .expect("the first send never reached the push config store");
    stalling.armed.store(false, Ordering::SeqCst);
    let doomed = stalling
        .stalled
        .lock()
        .expect("lock")
        .clone()
        .expect("stalled");
    first.abort();
    let _ = first.await;

    let mut second = tokio::spawn({
        let handler = Arc::clone(&handler);
        let mut params = into_context(keyed_with_push(None), "m-second");
        params.configuration = None;
        async move { handler.on_send_message(params, false, None).await }
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(300), &mut second)
            .await
            .is_err(),
        "the second send ran while the first's row was still being rolled back"
    );
    tasks.gate.add_permits(1);
    let answered = tokio::time::timeout(Duration::from_secs(10), second)
        .await
        .expect("the second send hung after the rollback")
        .expect("join")
        .expect("the second send runs");
    let id = match answered {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    };
    assert_ne!(
        id, doomed,
        "the second send continued the task being deleted"
    );
    assert_eq!(task_ids(&*tasks, "").await, [id]);
}
