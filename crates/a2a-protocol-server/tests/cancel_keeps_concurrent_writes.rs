// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Release audit F6: `CancelTask` writes the status and nothing else.
//!
//! Cancel re-reads the task, sets `Canceled` and writes it back. It wrote the
//! *whole* snapshot back with `save`, so anything another replica appended in
//! between — a history message, an artifact part — was erased. The write is
//! now `save_status_delta`, which every shipped store applies in place and
//! still refuses on a terminal task.
//!
//! The interleaving is forced without sleeps: the store wrapper waits until
//! the executor's cancel hook has run, then, immediately before cancel's own
//! write of `Canceled`, appends a message as another replica would *after*
//! cancel took the snapshot it is about to write over.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::store::{HistoryLimit, InMemoryTaskStore, TaskStore};
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder, SendMessageResult};
use a2a_protocol_types::Message;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::params::{CancelTaskParams, ListTasksParams, MessageSendParams};
use a2a_protocol_types::responses::{SendMessageResponse, TaskListResponse};
use a2a_protocol_types::task::{Task, TaskId, TaskState};

/// Parks the task at `INPUT_REQUIRED`; its cancel hook only says it ran.
struct Parks(Arc<AtomicBool>);

impl AgentExecutor for Parks {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            EventEmitter::new(ctx, queue)
                .status(TaskState::InputRequired)
                .await
        })
    }

    fn cancel<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.store(true, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

/// Once the cancel hook has run, the first write of `Canceled` is preceded by
/// a message appended to the stored task: another replica's write landing
/// between cancel's read and its write.
///
/// The trigger is cancel's own write, not "the next read". A read is not
/// cancel's alone: the `SendMessage` call that parked the task finishes in
/// the background (`SyncCollector::close_out` reads the task once more), and
/// on a store whose reads yield, such as `SQLite`, that read can fall after the
/// cancel hook and take a read-triggered append for itself, leaving cancel's
/// re-read before the message (measured: 17 of 200 runs). Only cancel writes
/// `Canceled`, so the interleaving is the same on every run and every store.
struct AppendsBeforeCancelsWrite {
    inner: Arc<dyn TaskStore>,
    hook_ran: Arc<AtomicBool>,
}

impl AppendsBeforeCancelsWrite {
    /// Writes the other replica's message first when `task` is cancel's write.
    async fn interleave(&self, task: &Task) -> A2aResult<()> {
        if task.status.state == TaskState::Canceled && self.hook_ran.swap(false, Ordering::SeqCst) {
            let stored = self.inner.get(&task.id).await?.expect("task is stored");
            self.inner
                .save_appending_history(
                    &stored,
                    &[Message::agent_text("from-replica-b", "written meanwhile")],
                    HistoryLimit::new(1024, usize::MAX),
                )
                .await?;
        }
        Ok(())
    }
}

impl TaskStore for AppendsBeforeCancelsWrite {
    fn save<'a>(
        &'a self,
        task: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.interleave(task).await?;
            self.inner.save(task).await
        })
    }
    fn save_status_delta<'a>(
        &'a self,
        task: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.interleave(task).await?;
            self.inner.save_status_delta(task).await
        })
    }
    fn try_save_status_delta<'a>(
        &'a self,
        task: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        Box::pin(async move {
            self.interleave(task).await?;
            self.inner.try_save_status_delta(task).await
        })
    }
    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        self.inner.get(id)
    }
    fn list<'a>(
        &'a self,
        params: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
        self.inner.list(params)
    }
    fn insert_if_absent<'a>(
        &'a self,
        task: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.inner.insert_if_absent(task)
    }
    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.delete(id)
    }
}

async fn cancel_keeps_the_concurrent_write(inner: Arc<dyn TaskStore>) {
    let hook_ran = Arc::new(AtomicBool::new(false));
    let handler = RequestHandlerBuilder::new(Parks(Arc::clone(&hook_ran)))
        .without_agent_card()
        .with_task_store(AppendsBeforeCancelsWrite {
            inner: Arc::clone(&inner),
            hook_ran,
        })
        .build()
        .expect("handler");
    let task_id = match handler
        .on_send_message(
            MessageSendParams::new(Message::user_text("m1", "start")),
            false,
            None,
        )
        .await
        .expect("send")
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => {
            assert_eq!(t.status.state, TaskState::InputRequired);
            t.id
        }
        other => panic!("expected a task, got {other:?}"),
    };

    let canceled = handler
        .on_cancel_task(
            CancelTaskParams {
                tenant: None,
                id: task_id.0.clone(),
                metadata: None,
            },
            None,
        )
        .await
        .expect("cancel");
    assert_eq!(canceled.status.state, TaskState::Canceled);

    let stored = inner.get(&task_id).await.expect("get").expect("stored");
    assert_eq!(stored.status.state, TaskState::Canceled);
    let ids: Vec<&str> = stored
        .history
        .iter()
        .flatten()
        .map(|m| m.id.0.as_str())
        .collect();
    assert!(
        ids.contains(&"from-replica-b"),
        "cancel's write erased the message written meanwhile: {ids:?}"
    );
}

#[tokio::test]
async fn in_memory() {
    cancel_keeps_the_concurrent_write(Arc::new(InMemoryTaskStore::new())).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite() {
    let path = std::env::temp_dir().join(format!("a2a-cancel-delta-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store =
        a2a_protocol_server::store::SqliteTaskStore::new(&format!("sqlite://{}", path.display()))
            .await
            .expect("store");
    cancel_keeps_the_concurrent_write(Arc::new(store)).await;
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

/// `#[ignore]`d without a live server, like the other PostgreSQL suites:
/// `A2A_TEST_POSTGRES_URL=… cargo test --features postgres --test
/// cancel_keeps_concurrent_writes -- --include-ignored`.
#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs A2A_TEST_POSTGRES_URL"]
async fn postgres() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let admin = sqlx::postgres::PgPool::connect(&url)
        .await
        .expect("connect");
    let name = format!("a2a_cancel_delta_{}", std::process::id());
    let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .expect("create database");
    let db_url = format!("{}/{name}", url.rsplit_once('/').expect("path").0);
    let store = a2a_protocol_server::store::PostgresTaskStore::new(&db_url)
        .await
        .expect("store");
    cancel_keeps_the_concurrent_write(Arc::new(store)).await;
    let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await;
    admin.close().await;
}
