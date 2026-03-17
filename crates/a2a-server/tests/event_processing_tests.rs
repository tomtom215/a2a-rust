// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Integration tests for event processing in `RequestHandler`.
//!
//! Tests cover `collect_events` (sync mode), `spawn_background_event_processor`
//! (streaming mode), push notification delivery, and error handling.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::push::PushSender;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::{EventQueueReader, EventQueueWriter};

// ── Test executors ──────────────────────────────────────────────────────────

/// Executor that emits Working -> Completed status updates.
struct StatusExecutor;

impl AgentExecutor for StatusExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

/// Executor that emits Working, an artifact, then Completed.
struct ArtifactExecutor;

impl AgentExecutor for ArtifactExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("art-1", vec![Part::text("artifact content")]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

/// Executor that emits a full Task event (replacing the task in the store).
struct TaskEventExecutor;

impl AgentExecutor for TaskEventExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let replacement = Task {
                id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                status: TaskStatus::new(TaskState::Completed),
                history: None,
                artifacts: Some(vec![Artifact::new(
                    "replaced-art",
                    vec![Part::text("replaced")],
                )]),
                metadata: Some(serde_json::json!({"replaced": true})),
            };
            queue.write(StreamResponse::Task(replacement)).await?;
            Ok(())
        })
    }
}

/// Executor that returns an error (gets written as Failed status by the spawned task).
struct ErrorExecutor;

impl AgentExecutor for ErrorExecutor {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Err(A2aError::internal("something went wrong")) })
    }
}

/// Executor that emits a Message event (should be a no-op for task state).
struct MessageEventExecutor;

impl AgentExecutor for MessageEventExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            let msg = Message {
                id: MessageId::new("msg-event"),
                role: MessageRole::Agent,
                parts: vec![Part::text("hello from agent")],
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            };
            queue.write(StreamResponse::Message(msg)).await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

// ── Recording push sender ───────────────────────────────────────────────────

/// A [`PushSender`] that records calls to a shared vec for assertion.
struct SharedRecordingPushSender {
    calls: Arc<Mutex<Vec<String>>>,
}

impl PushSender for SharedRecordingPushSender {
    fn send<'a>(
        &'a self,
        url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(url.to_string());
            Ok(())
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn make_send_params() -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::text("hello")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }
}

fn default_list_params() -> a2a_protocol_types::params::ListTasksParams {
    a2a_protocol_types::params::ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    }
}

fn extract_task(result: SendMessageResult) -> Task {
    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => task,
        _ => panic!("expected SendMessageResult::Response(Task)"),
    }
}

// ── Sync mode tests (collect_events) ────────────────────────────────────────

#[tokio::test]
async fn sync_mode_status_updates_stored() {
    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
}

#[tokio::test]
async fn sync_mode_artifact_updates_appended() {
    let handler = RequestHandlerBuilder::new(ArtifactExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
    let artifacts = task.artifacts.expect("task should have artifacts");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id.0, "art-1");
}

#[tokio::test]
async fn sync_mode_task_event_replaces_task() {
    let handler = RequestHandlerBuilder::new(TaskEventExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
    let artifacts = task.artifacts.expect("replaced task should have artifacts");
    assert_eq!(artifacts[0].id.0, "replaced-art");
    let meta = task.metadata.expect("replaced task should have metadata");
    assert_eq!(meta["replaced"], true);
}

#[tokio::test]
async fn sync_mode_error_marks_failed() {
    let handler = RequestHandlerBuilder::new(ErrorExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await;

    match result {
        Ok(send_result) => {
            let task = extract_task(send_result);
            assert_eq!(task.status.state, TaskState::Failed);
        }
        Err(_) => {
            // Error propagation is acceptable.
        }
    }
}

#[tokio::test]
async fn sync_mode_message_event_is_noop_for_state() {
    let handler = RequestHandlerBuilder::new(MessageEventExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
}

#[tokio::test]
async fn sync_mode_no_push_calls_when_no_sender() {
    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
}

#[tokio::test]
async fn sync_mode_no_push_calls_when_no_config() {
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .with_push_sender(SharedRecordingPushSender {
            calls: calls.clone(),
        })
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send message");

    let _task = extract_task(result);
    assert_eq!(calls.lock().unwrap().len(), 0);
}

// ── Streaming mode tests ────────────────────────────────────────────────────

#[tokio::test]
async fn streaming_mode_delivers_status_events() {
    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming message");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut states = vec![];
    while let Some(event) = reader.read().await {
        if let Ok(StreamResponse::StatusUpdate(u)) = event {
            states.push(u.status.state);
        }
    }

    assert!(states.contains(&TaskState::Working));
    assert!(states.contains(&TaskState::Completed));
}

#[tokio::test]
async fn streaming_mode_delivers_artifact_events() {
    let handler = RequestHandlerBuilder::new(ArtifactExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming message");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut artifact_count = 0;
    let mut states = vec![];
    while let Some(event) = reader.read().await {
        match event {
            Ok(StreamResponse::ArtifactUpdate(_)) => artifact_count += 1,
            Ok(StreamResponse::StatusUpdate(u)) => states.push(u.status.state),
            _ => {}
        }
    }

    assert_eq!(artifact_count, 1);
    assert!(states.contains(&TaskState::Working));
    assert!(states.contains(&TaskState::Completed));
}

#[tokio::test]
async fn streaming_mode_error_produces_failed_event() {
    let handler = RequestHandlerBuilder::new(ErrorExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming message");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut saw_failed = false;
    while let Some(event) = reader.read().await {
        if let Ok(StreamResponse::StatusUpdate(u)) = event {
            if u.status.state == TaskState::Failed {
                saw_failed = true;
            }
        }
    }

    assert!(saw_failed, "should see Failed status event in stream");
}

#[tokio::test]
async fn streaming_mode_receives_all_events() {
    let handler = RequestHandlerBuilder::new(ArtifactExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming message");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut event_count = 0;
    while let Some(event) = reader.read().await {
        if event.is_ok() {
            event_count += 1;
        }
    }
    assert!(
        event_count >= 3,
        "should receive at least 3 events, got {event_count}"
    );
}

// ── Background processor + push delivery (cooperative executor) ─────────────
//
// These tests use a cooperative executor that waits for a signal before
// writing events. This avoids the race condition where the executor finishes
// before the background processor subscribes to the event queue.

#[tokio::test]
async fn streaming_mode_background_processor_updates_store() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    struct WaitingExecutor {
        proceed: Arc<Notify>,
        started: Arc<AtomicBool>,
    }

    impl AgentExecutor for WaitingExecutor {
        fn execute<'a>(
            &'a self,
            ctx: &'a RequestContext,
            queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async move {
                self.started.store(true, Ordering::SeqCst);
                self.proceed.notified().await;
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(TaskState::Working),
                        metadata: None,
                    }))
                    .await?;
                queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact: Artifact::new("art-bg", vec![Part::text("background")]),
                        append: None,
                        last_chunk: Some(true),
                        metadata: None,
                    }))
                    .await?;
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(TaskState::Completed),
                        metadata: None,
                    }))
                    .await?;
                Ok(())
            })
        }
    }

    let proceed = Arc::new(Notify::new());
    let started = Arc::new(AtomicBool::new(false));

    let handler = Arc::new(
        RequestHandlerBuilder::new(WaitingExecutor {
            proceed: proceed.clone(),
            started: started.clone(),
        })
        .build()
        .expect("build handler"),
    );

    let handler_clone = handler.clone();
    let send_handle = tokio::spawn(async move {
        let result = handler_clone
            .on_send_message(make_send_params(), true, None)
            .await
            .expect("send message");
        match result {
            SendMessageResult::Stream(mut reader) => {
                while let Some(_event) = reader.read().await {}
            }
            _ => panic!("expected Stream"),
        }
    });

    while !started.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    // Give background processor time to subscribe.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    proceed.notify_one();
    send_handle.await.expect("send handle");

    // Give background processor time to process all events.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let list = handler
        .on_list_tasks(default_list_params(), None)
        .await
        .expect("list tasks");

    assert!(!list.tasks.is_empty());
    let completed = list
        .tasks
        .iter()
        .find(|t| t.status.state == TaskState::Completed);
    assert!(completed.is_some());
    let task = completed.unwrap();
    assert!(task.artifacts.is_some());
    assert_eq!(task.artifacts.as_ref().unwrap().len(), 1);
}

#[tokio::test]
async fn streaming_mode_push_delivery_with_cooperative_executor() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    struct WaitingExecutor {
        proceed: Arc<Notify>,
        started: Arc<AtomicBool>,
    }

    impl AgentExecutor for WaitingExecutor {
        fn execute<'a>(
            &'a self,
            ctx: &'a RequestContext,
            queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async move {
                self.started.store(true, Ordering::SeqCst);
                self.proceed.notified().await;
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(TaskState::Working),
                        metadata: None,
                    }))
                    .await?;
                queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact: Artifact::new("art-coop", vec![Part::text("coop")]),
                        append: None,
                        last_chunk: None,
                        metadata: None,
                    }))
                    .await?;
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(TaskState::Completed),
                        metadata: None,
                    }))
                    .await?;
                Ok(())
            })
        }
    }

    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let proceed = Arc::new(Notify::new());
    let started = Arc::new(AtomicBool::new(false));

    let handler = Arc::new(
        RequestHandlerBuilder::new(WaitingExecutor {
            proceed: proceed.clone(),
            started: started.clone(),
        })
        .with_push_sender(SharedRecordingPushSender {
            calls: calls.clone(),
        })
        .build()
        .expect("build handler"),
    );

    let handler_clone = handler.clone();
    let send_handle = tokio::spawn(async move {
        let result = handler_clone
            .on_send_message(make_send_params(), true, None)
            .await
            .expect("send message");
        match result {
            SendMessageResult::Stream(mut reader) => {
                while let Some(_event) = reader.read().await {}
            }
            _ => panic!("expected Stream"),
        }
    });

    while !started.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let list = handler
        .on_list_tasks(default_list_params(), None)
        .await
        .expect("list tasks");
    assert!(!list.tasks.is_empty());
    let task_id = list.tasks[0].id.0.clone();

    let config = TaskPushNotificationConfig::new(&task_id, "https://example.com/push-test");
    handler
        .on_set_push_config(config, None)
        .await
        .expect("set push config");

    proceed.notify_one();
    send_handle.await.expect("send handle");

    // Give background processor time to deliver push notifications.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let count = calls.lock().unwrap().len();
    assert!(
        count >= 2,
        "expected at least 2 push calls (status + artifact), got {count}"
    );
    let urls = calls.lock().unwrap().clone();
    assert!(urls.iter().all(|u| u == "https://example.com/push-test"));
}

// ── Invalid state transition tests ──────────────────────────────────────────

/// Executor that emits Working then attempts Submitted (invalid transition).
struct InvalidTransitionExecutor;

impl AgentExecutor for InvalidTransitionExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            // Working -> Submitted is invalid.
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Submitted),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn sync_mode_invalid_state_transition_returns_error() {
    let handler = RequestHandlerBuilder::new(InvalidTransitionExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await;

    match result {
        Err(ref err) => {
            assert!(
                matches!(
                    err,
                    a2a_protocol_server::ServerError::InvalidStateTransition { .. }
                ),
                "expected InvalidStateTransition, got {err:?}"
            );
        }
        Ok(_) => panic!("expected error for invalid state transition"),
    }
}

/// Executor that reaches Completed, then tries Working (terminal -> non-terminal).
struct TerminalTransitionExecutor;

impl AgentExecutor for TerminalTransitionExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            // Completed -> Working is invalid (terminal state).
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn sync_mode_completed_to_working_is_invalid() {
    let handler = RequestHandlerBuilder::new(TerminalTransitionExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await;

    match result {
        Err(ref err) => {
            assert!(
                matches!(
                    err,
                    a2a_protocol_server::ServerError::InvalidStateTransition { .. }
                ),
                "expected InvalidStateTransition, got {err:?}"
            );
        }
        Ok(_) => panic!("expected error for terminal state transition"),
    }
}

#[tokio::test]
async fn streaming_mode_invalid_transition_does_not_crash_stream() {
    let handler = RequestHandlerBuilder::new(InvalidTransitionExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut events = vec![];
    while let Some(event) = reader.read().await {
        events.push(event);
    }

    // The SSE reader still sees all events (the invalid transition is only
    // rejected by the background processor, not the SSE layer).
    assert!(!events.is_empty(), "stream should still produce events");
}

// ── Executor error tests (additional) ────────────────────────────────────────

/// Executor that emits Working, then returns an error.
struct WorkThenErrorExecutor;

impl AgentExecutor for WorkThenErrorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            Err(A2aError::internal("executor failed mid-flight"))
        })
    }
}

#[tokio::test]
async fn sync_mode_work_then_error_marks_failed() {
    let handler = RequestHandlerBuilder::new(WorkThenErrorExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await;

    match result {
        Ok(send_result) => {
            let task = extract_task(send_result);
            assert_eq!(
                task.status.state,
                TaskState::Failed,
                "executor error after Working should leave task in Failed state"
            );
        }
        Err(_) => {
            // Error propagation is also acceptable.
        }
    }
}

#[tokio::test]
async fn streaming_mode_work_then_error_marks_failed_in_store() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    struct WaitingWorkThenErrorExecutor {
        proceed: Arc<Notify>,
        started: Arc<AtomicBool>,
    }

    impl AgentExecutor for WaitingWorkThenErrorExecutor {
        fn execute<'a>(
            &'a self,
            ctx: &'a RequestContext,
            queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async move {
                self.started.store(true, Ordering::SeqCst);
                self.proceed.notified().await;
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(TaskState::Working),
                        metadata: None,
                    }))
                    .await?;
                Err(A2aError::internal("deliberate error after working"))
            })
        }
    }

    let proceed = Arc::new(Notify::new());
    let started = Arc::new(AtomicBool::new(false));

    let handler = Arc::new(
        RequestHandlerBuilder::new(WaitingWorkThenErrorExecutor {
            proceed: proceed.clone(),
            started: started.clone(),
        })
        .build()
        .expect("build handler"),
    );

    let handler_clone = handler.clone();
    let send_handle = tokio::spawn(async move {
        let result = handler_clone
            .on_send_message(make_send_params(), true, None)
            .await
            .expect("send message");
        match result {
            SendMessageResult::Stream(mut reader) => {
                let mut saw_working = false;
                let mut saw_failed = false;
                while let Some(event) = reader.read().await {
                    match event {
                        Ok(StreamResponse::StatusUpdate(u))
                            if u.status.state == TaskState::Working =>
                        {
                            saw_working = true;
                        }
                        Ok(StreamResponse::StatusUpdate(u))
                            if u.status.state == TaskState::Failed =>
                        {
                            saw_failed = true;
                        }
                        _ => {}
                    }
                }
                assert!(saw_working, "should have seen Working status");
                assert!(saw_failed, "should have seen Failed status");
            }
            _ => panic!("expected Stream"),
        }
    });

    while !started.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    proceed.notify_one();
    send_handle.await.expect("send handle");

    // Poll until background processor updates the store.
    let mut found = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let list = handler
            .on_list_tasks(default_list_params(), None)
            .await
            .expect("list tasks");
        if list
            .tasks
            .iter()
            .any(|t| t.status.state == TaskState::Failed)
        {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "background processor should mark task as Failed after executor error"
    );
}

// ── Push delivery timeout test ──────────────────────────────────────────────

/// A push sender that sleeps forever (for timeout testing).
struct SleepForeverPushSender;

impl PushSender for SleepForeverPushSender {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            Ok(())
        })
    }
}

#[tokio::test]
async fn sync_mode_push_delivery_timeout_does_not_block_forever() {
    use a2a_protocol_server::handler::HandlerLimits;

    let limits =
        HandlerLimits::default().with_push_delivery_timeout(std::time::Duration::from_millis(50));

    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .with_push_sender(SleepForeverPushSender)
        .with_handler_limits(limits)
        .build()
        .expect("build handler");

    // The push sender sleeps forever, but the delivery timeout is 50ms.
    let start = std::time::Instant::now();
    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send should not hang");
    let elapsed = start.elapsed();

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "handler should not block for long; elapsed: {elapsed:?}"
    );
}

// ── Additional transition tests ─────────────────────────────────────────────

/// Executor that exercises: Working -> InputRequired -> Working -> Completed.
struct MultiTransitionExecutor;

impl AgentExecutor for MultiTransitionExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            for state in [
                TaskState::Working,
                TaskState::InputRequired,
                TaskState::Working,
                TaskState::Completed,
            ] {
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(state),
                        metadata: None,
                    }))
                    .await?;
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn sync_mode_multiple_valid_transitions() {
    let handler = RequestHandlerBuilder::new(MultiTransitionExecutor)
        .build()
        .expect("build handler");

    let task = extract_task(
        handler
            .on_send_message(make_send_params(), false, None)
            .await
            .expect("send"),
    );
    assert_eq!(task.status.state, TaskState::Completed);
}

/// Executor that emits Working -> Canceled.
struct CanceledExecutor;

impl AgentExecutor for CanceledExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Canceled),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn sync_mode_working_to_canceled() {
    let handler = RequestHandlerBuilder::new(CanceledExecutor)
        .build()
        .expect("build handler");

    let task = extract_task(
        handler
            .on_send_message(make_send_params(), false, None)
            .await
            .expect("send"),
    );
    assert_eq!(task.status.state, TaskState::Canceled);
}

/// Executor that emits Working -> Failed (via status update, not error).
struct FailedStatusExecutor;

impl AgentExecutor for FailedStatusExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Failed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn sync_mode_working_to_failed_via_status_update() {
    let handler = RequestHandlerBuilder::new(FailedStatusExecutor)
        .build()
        .expect("build handler");

    let task = extract_task(
        handler
            .on_send_message(make_send_params(), false, None)
            .await
            .expect("send"),
    );
    assert_eq!(task.status.state, TaskState::Failed);
}

// ── Streaming: error marks Failed in store ──────────────────────────────────

#[tokio::test]
async fn streaming_mode_error_marks_failed_in_store() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    struct WaitingErrorExecutor {
        proceed: Arc<Notify>,
        started: Arc<AtomicBool>,
    }

    impl AgentExecutor for WaitingErrorExecutor {
        fn execute<'a>(
            &'a self,
            _ctx: &'a RequestContext,
            _queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async move {
                self.started.store(true, Ordering::SeqCst);
                self.proceed.notified().await;
                Err(A2aError::internal("deliberate error"))
            })
        }
    }

    let proceed = Arc::new(Notify::new());
    let started = Arc::new(AtomicBool::new(false));

    let handler = Arc::new(
        RequestHandlerBuilder::new(WaitingErrorExecutor {
            proceed: proceed.clone(),
            started: started.clone(),
        })
        .build()
        .expect("build handler"),
    );

    let handler_clone = handler.clone();
    let send_handle = tokio::spawn(async move {
        let result = handler_clone
            .on_send_message(make_send_params(), true, None)
            .await
            .expect("send message");
        match result {
            SendMessageResult::Stream(mut reader) => {
                while let Some(_event) = reader.read().await {}
            }
            _ => panic!("expected Stream"),
        }
    });

    while !started.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    proceed.notify_one();
    send_handle.await.expect("send handle");

    // Poll until background processor updates the store.
    let mut found = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let list = handler
            .on_list_tasks(default_list_params(), None)
            .await
            .expect("list tasks");
        if list
            .tasks
            .iter()
            .any(|t| t.status.state == TaskState::Failed)
        {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "background processor should mark task as Failed after error event"
    );
}

// ── Streaming: message event passes through ─────────────────────────────────

#[tokio::test]
async fn streaming_mode_message_event_passes_through() {
    let handler = RequestHandlerBuilder::new(MessageEventExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut saw_message = false;
    let mut states = vec![];
    while let Some(event) = reader.read().await {
        match event {
            Ok(StreamResponse::Message(_)) => saw_message = true,
            Ok(StreamResponse::StatusUpdate(u)) => states.push(u.status.state),
            _ => {}
        }
    }

    assert!(saw_message, "should have seen Message event in stream");
    assert_eq!(states, vec![TaskState::Working, TaskState::Completed]);
}

// ── Streaming: task snapshot event passes through ───────────────────────────

#[tokio::test]
async fn streaming_mode_task_snapshot_in_stream() {
    let handler = RequestHandlerBuilder::new(TaskEventExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut saw_task = false;
    while let Some(event) = reader.read().await {
        if let Ok(StreamResponse::Task(_)) = event {
            saw_task = true;
        }
    }

    assert!(saw_task, "should have seen Task snapshot in stream");
}

// ── Background processor drains events after executor finishes ──────────────

#[tokio::test]
async fn streaming_mode_background_drains_after_executor_done() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    struct WaitingArtifactExecutor {
        proceed: Arc<Notify>,
        started: Arc<AtomicBool>,
    }

    impl AgentExecutor for WaitingArtifactExecutor {
        fn execute<'a>(
            &'a self,
            ctx: &'a RequestContext,
            queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async move {
                self.started.store(true, Ordering::SeqCst);
                self.proceed.notified().await;
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(TaskState::Working),
                        metadata: None,
                    }))
                    .await?;
                queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact: Artifact::new("art-1", vec![Part::text("artifact content")]),
                        append: None,
                        last_chunk: Some(true),
                        metadata: None,
                    }))
                    .await?;
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(TaskState::Completed),
                        metadata: None,
                    }))
                    .await?;
                Ok(())
            })
        }
    }

    let proceed = Arc::new(Notify::new());
    let started = Arc::new(AtomicBool::new(false));

    let handler = Arc::new(
        RequestHandlerBuilder::new(WaitingArtifactExecutor {
            proceed: proceed.clone(),
            started: started.clone(),
        })
        .build()
        .expect("build handler"),
    );

    let handler_clone = handler.clone();
    let send_handle = tokio::spawn(async move {
        let result = handler_clone
            .on_send_message(make_send_params(), true, None)
            .await
            .expect("send message");
        match result {
            SendMessageResult::Stream(mut reader) => {
                let mut event_count = 0;
                while let Some(event) = reader.read().await {
                    if event.is_ok() {
                        event_count += 1;
                    }
                }
                assert!(
                    event_count >= 3,
                    "should receive >= 3 events, got {event_count}"
                );
            }
            _ => panic!("expected Stream"),
        }
    });

    while !started.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    proceed.notify_one();
    send_handle.await.expect("send handle");

    // Poll until background processor updates the store.
    let mut found = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let list = handler
            .on_list_tasks(default_list_params(), None)
            .await
            .expect("list tasks");
        if list
            .tasks
            .iter()
            .any(|t| t.status.state == TaskState::Completed && t.artifacts.is_some())
        {
            found = true;
            break;
        }
    }
    assert!(found, "background processor should have drained all events");
}

// ── Empty executor returns initial state ────────────────────────────────────

/// Executor that emits no events.
struct EmptyExecutor;

impl AgentExecutor for EmptyExecutor {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

#[tokio::test]
async fn sync_mode_empty_executor_returns_submitted() {
    let handler = RequestHandlerBuilder::new(EmptyExecutor)
        .build()
        .expect("build handler");

    let task = extract_task(
        handler
            .on_send_message(make_send_params(), false, None)
            .await
            .expect("send"),
    );
    assert_eq!(
        task.status.state,
        TaskState::Submitted,
        "no events emitted; task should stay Submitted"
    );
}
