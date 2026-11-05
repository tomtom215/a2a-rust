// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A turn handed back on shutdown (ADR 0054, audit D-21): left `working`
//! and marked, its claim released, and run again by another replica when its
//! caller sends the turn's message again to the task.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::failure::{FailureClass, class_of};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{Task, TaskId, TaskState};

use crate::builder::RequestHandlerBuilder;
use crate::executor::{AgentExecutor, HAND_OFF_METADATA_KEY};
use crate::request_context::RequestContext;
use crate::store::TaskStore;
use crate::streaming::EventQueueWriter;
use crate::{EventEmitter, RequestHandler, SendMessageResult};

/// Works until its token fires, then returns with no terminal state; run on
/// a task a shutdown handed back, it completes at once.
struct Resumable(bool);

impl AgentExecutor for Resumable {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            let handed_back = ctx.stored_task.as_ref().is_some_and(marked);
            if handed_back {
                return emit.status(TaskState::Completed).await;
            }
            emit.status(TaskState::Working).await?;
            ctx.cancellation_token.cancelled().await;
            Ok(())
        })
    }

    fn resumes_after_hand_off(&self) -> bool {
        self.0
    }
}

fn marked(task: &Task) -> bool {
    task.status
        .message
        .as_ref()
        .and_then(|m| m.metadata.as_ref())
        .and_then(|m| m.get(HAND_OFF_METADATA_KEY))
        == Some(&serde_json::Value::Bool(true))
}

fn handler(store: &Arc<dyn TaskStore>, resumes: bool) -> RequestHandler {
    RequestHandlerBuilder::new(Resumable(resumes))
        .with_task_store_arc(Arc::clone(store))
        .without_agent_card()
        .build()
        .unwrap()
}

fn send(task: Option<&str>) -> MessageSendParams {
    let mut message = Message::new(
        MessageId::new(uuid::Uuid::new_v4().to_string()),
        MessageRole::User,
        vec![Part::text("work")],
    );
    message.task_id = task.map(TaskId::new);
    let mut params = MessageSendParams::new(message);
    params.configuration =
        Some(serde_json::from_value(serde_json::json!({ "returnImmediately": true })).unwrap());
    params
}

async fn started(h: &RequestHandler) -> String {
    match h.on_send_message(send(None), false, None).await.unwrap() {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    }
}

async fn reaches(store: &Arc<dyn TaskStore>, id: &str, want: TaskState) -> Task {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let task = store.get(&TaskId::new(id)).await.unwrap().unwrap();
        if task.status.state == want {
            return task;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{id} is {:?}, never {want:?}",
            task.status.state
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A database file of its own, and its name.
#[cfg(feature = "sqlite")]
fn sqlite_url() -> (String, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("a2a-hand-off-{}.db", uuid::Uuid::new_v4()));
    (format!("sqlite:{}?mode=rwc", path.display()), path)
}

/// Removes the database and the files the pool leaves beside it.
#[cfg(feature = "sqlite")]
fn remove_sqlite(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(std::path::PathBuf::from(name));
    }
}

/// On a shared store with claims, a resumable turn stopped by shutdown is
/// left `working` with the hand-off mark, its cancel hook not run (no
/// `canceled`), its claim released; another replica runs the caller's next
/// send to the task to completion.
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn a_resumable_turn_is_handed_off_and_another_replica_runs_it() {
    let (url, path) = sqlite_url();
    let store: Arc<dyn TaskStore> =
        Arc::new(crate::store::SqliteTaskStore::new(&url).await.unwrap());
    let a = handler(&store, true);
    let id = started(&a).await;
    reaches(&store, &id, TaskState::Working).await;

    let report = a.cancel_in_flight(Duration::from_secs(5)).await;
    assert!(report.finished, "{report:?}");
    let handed = store.get(&TaskId::new(&*id)).await.unwrap().unwrap();
    assert_eq!(handed.status.state, TaskState::Working);
    assert!(marked(&handed), "{:?}", handed.status);

    let b = handler(&store, true);
    b.on_send_message(send(Some(&id)), false, None)
        .await
        .expect("another replica admits the continuation");
    reaches(&store, &id, TaskState::Completed).await;
    drop((a, b));
    remove_sqlite(&path);
}

/// An executor that does not opt in, and one that does on a handler
/// without claims (no other replica could take the task), end
/// `Failed`/`transient` as before.
#[tokio::test]
async fn without_opt_in_or_claims_a_shutdown_still_fails_the_turn() {
    let in_memory: Arc<dyn TaskStore> = Arc::new(crate::store::InMemoryTaskStore::new());
    let mut cases = vec![(in_memory, true, None)];
    #[cfg(feature = "sqlite")]
    {
        let (url, path) = sqlite_url();
        let shared: Arc<dyn TaskStore> =
            Arc::new(crate::store::SqliteTaskStore::new(&url).await.unwrap());
        cases.push((shared, false, Some(path)));
    }
    for (store, resumes, path) in cases {
        let h = handler(&store, resumes);
        let id = started(&h).await;
        reaches(&store, &id, TaskState::Working).await;
        h.cancel_in_flight(Duration::from_secs(5)).await;
        let ended = reaches(&store, &id, TaskState::Failed).await;
        let note = ended.status.message.as_ref().expect("a reason");
        assert_eq!(class_of(note), Some(FailureClass::Transient));
        assert!(!marked(&ended));
        drop(h);
        if let Some(path) = path {
            remove_sqlite(&path);
        }
    }
}

/// The SQL statements spell the window as 600 seconds.
#[test]
fn the_hand_off_window_is_the_one_the_sql_spells() {
    assert_eq!(crate::executor::HAND_OFF_WINDOW, Duration::from_secs(600));
}
