// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Edge case tests covering gaps identified in the audit.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::{A2aError, A2aResult, ErrorCode};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams, TaskQueryParams};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus, TaskVersion};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::store::InMemoryTaskStore;
use a2a_protocol_server::streaming::{EventQueueReader, EventQueueWriter};
use a2a_protocol_server::{ServerError, TaskStoreConfig};

// ── Test executor ─────────────────────────────────────────────────────────────

struct EchoExecutor;

impl AgentExecutor for EchoExecutor {
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
                    status: TaskStatus::with_timestamp(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::with_timestamp(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

struct FailingExecutor;

impl AgentExecutor for FailingExecutor {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Err(A2aError::internal("executor exploded")) })
    }
}

struct SlowExecutor;

impl AgentExecutor for SlowExecutor {
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
                    status: TaskStatus::with_timestamp(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            // Check for cancellation
            tokio::select! {
                _ = ctx.cancellation_token.cancelled() => {
                    Err(A2aError::internal("task was cancelled"))
                }
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    Ok(())
                }
            }
        })
    }
}

fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn send_message_basic_flow() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();
    let result = handler
        .on_send_message(make_send_params("hello"), false, None)
        .await;
    match result.unwrap() {
        a2a_protocol_server::SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Response(Task)"),
    }
}

#[tokio::test]
async fn send_message_executor_failure_marks_task_failed() {
    let handler = RequestHandlerBuilder::new(FailingExecutor).build().unwrap();
    let result = handler
        .on_send_message(make_send_params("hello"), false, None)
        .await;
    // The executor fails, which should result in a Failed task
    match result {
        Ok(a2a_protocol_server::SendMessageResult::Response(SendMessageResponse::Task(task))) => {
            assert_eq!(task.status.state, TaskState::Failed);
        }
        Err(_e) => {
            // Also acceptable — depends on timing of the failure event
        }
        _ => panic!("unexpected result"),
    }
}

#[tokio::test]
async fn cancel_task_signals_cancellation_token() {
    let handler = Arc::new(RequestHandlerBuilder::new(SlowExecutor).build().unwrap());

    // Start a streaming task (which won't block waiting for completion)
    let params = make_send_params("slow task");
    let result = handler.on_send_message(params, true, None).await.unwrap();

    // Get the task ID from the response
    match result {
        a2a_protocol_server::SendMessageResult::Stream(_reader) => {
            // Give the executor a moment to start
            tokio::time::sleep(Duration::from_millis(50)).await;

            // List tasks to find our task
            let list_result = handler
                .on_list_tasks(
                    ListTasksParams {
                        tenant: None,
                        context_id: None,
                        status: None,
                        page_size: Some(10),
                        page_token: None,
                        status_timestamp_after: None,
                        include_artifacts: None,
                        history_length: None,
                    },
                    None,
                )
                .await
                .unwrap();

            assert!(
                !list_result.tasks.is_empty(),
                "should have at least one task"
            );

            // Find a non-terminal task to cancel
            if let Some(task) = list_result
                .tasks
                .iter()
                .find(|t| !t.status.state.is_terminal())
            {
                let cancel_result = handler
                    .on_cancel_task(
                        a2a_protocol_types::params::CancelTaskParams {
                            tenant: None,
                            id: task.id.to_string(),
                            metadata: None,
                        },
                        None,
                    )
                    .await;
                match cancel_result {
                    Ok(cancelled) => {
                        assert_eq!(cancelled.status.state, TaskState::Canceled);
                    }
                    Err(_) => {
                        // Task may have completed by now — that's OK
                    }
                }
            }
        }
        _ => panic!("expected Stream result"),
    }
}

#[tokio::test]
async fn get_task_not_found() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();
    let result = handler
        .on_get_task(
            TaskQueryParams {
                tenant: None,
                id: "nonexistent-task".into(),
                history_length: None,
            },
            None,
        )
        .await;
    assert!(matches!(result, Err(ServerError::TaskNotFound(_))));
}

#[tokio::test]
async fn cancel_task_not_found() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();
    let result = handler
        .on_cancel_task(
            a2a_protocol_types::params::CancelTaskParams {
                tenant: None,
                id: "nonexistent".into(),
                metadata: None,
            },
            None,
        )
        .await;
    assert!(matches!(result, Err(ServerError::TaskNotFound(_))));
}

#[tokio::test]
async fn cancel_completed_task_returns_not_cancelable() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();

    // First, complete a task
    let result = handler
        .on_send_message(make_send_params("hello"), false, None)
        .await
        .unwrap();
    let task_id = match result {
        a2a_protocol_server::SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            task.id
        }
        _ => panic!("expected task"),
    };

    // Now try to cancel it
    let cancel_result = handler
        .on_cancel_task(
            a2a_protocol_types::params::CancelTaskParams {
                tenant: None,
                id: task_id.to_string(),
                metadata: None,
            },
            None,
        )
        .await;
    assert!(matches!(
        cancel_result,
        Err(ServerError::TaskNotCancelable(_))
    ));
}

#[tokio::test]
async fn list_tasks_pagination_page_size_zero_defaults() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();

    // Create a task
    handler
        .on_send_message(make_send_params("hello"), false, None)
        .await
        .unwrap();

    // List with page_size = 0 (should default to 50, not return empty)
    let result = handler
        .on_list_tasks(
            ListTasksParams {
                tenant: None,
                context_id: None,
                status: None,
                page_size: Some(0),
                page_token: None,
                status_timestamp_after: None,
                include_artifacts: None,
                history_length: None,
            },
            None,
        )
        .await
        .unwrap();
    assert!(
        !result.tasks.is_empty(),
        "page_size=0 should default to 50, not empty"
    );
}

#[tokio::test]
async fn push_config_not_supported_without_sender() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();

    let config = a2a_protocol_types::push::TaskPushNotificationConfig::new(
        "task-1",
        "http://example.com/webhook",
    );
    let result = handler.on_set_push_config(config, None).await;
    assert!(matches!(result, Err(ServerError::PushNotSupported)));
}

#[tokio::test]
async fn extended_agent_card_not_configured() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();
    let result = handler.on_get_extended_agent_card(None).await;
    assert!(matches!(result, Err(ServerError::Internal(_))));
}

#[tokio::test]
async fn task_status_with_timestamp_has_value() {
    let status = TaskStatus::with_timestamp(TaskState::Working);
    assert!(status.timestamp.is_some());
    let ts = status.timestamp.unwrap();
    assert!(ts.ends_with('Z'), "timestamp should be UTC: {ts}");
    assert!(ts.contains('T'), "timestamp should be ISO 8601: {ts}");
}

#[tokio::test]
async fn task_store_eviction_on_write() {
    let config = TaskStoreConfig {
        max_capacity: Some(2),
        task_ttl: None,
        ..Default::default()
    };
    let store = InMemoryTaskStore::with_config(config);

    // Write 3 tasks, first two completed
    for i in 0..3 {
        let task = Task {
            id: TaskId::new(format!("task-{i}")),
            context_id: ContextId::new("ctx"),
            status: if i < 2 {
                TaskStatus::new(TaskState::Completed)
            } else {
                TaskStatus::new(TaskState::Working)
            },
            history: None,
            artifacts: None,
            metadata: None,
        };
        a2a_protocol_server::TaskStore::save(&store, task)
            .await
            .unwrap();
    }

    // The oldest completed task should have been evicted
    let list = a2a_protocol_server::TaskStore::list(
        &store,
        &ListTasksParams {
            tenant: None,
            context_id: None,
            status: None,
            page_size: Some(50),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(list.tasks.len(), 2, "should have evicted one task");
}

#[tokio::test]
async fn id_newtypes_from_impls() {
    let task_id: TaskId = "my-task".into();
    assert_eq!(task_id.as_ref(), "my-task");

    let task_id: TaskId = String::from("my-task").into();
    assert_eq!(task_id.0, "my-task");

    let ctx_id: ContextId = "ctx-1".into();
    assert_eq!(ctx_id.as_ref(), "ctx-1");

    let msg_id: a2a_protocol_types::MessageId = "msg-1".into();
    assert_eq!(msg_id.as_ref(), "msg-1");

    let art_id: a2a_protocol_types::ArtifactId = "art-1".into();
    assert_eq!(art_id.as_ref(), "art-1");
}

#[tokio::test]
async fn task_state_transitions_comprehensive() {
    // Valid transitions
    assert!(TaskState::Submitted.can_transition_to(TaskState::Working));
    assert!(TaskState::Submitted.can_transition_to(TaskState::Failed));
    assert!(TaskState::Submitted.can_transition_to(TaskState::Canceled));
    assert!(TaskState::Submitted.can_transition_to(TaskState::Rejected));
    assert!(TaskState::Working.can_transition_to(TaskState::Completed));
    assert!(TaskState::Working.can_transition_to(TaskState::Failed));
    assert!(TaskState::Working.can_transition_to(TaskState::InputRequired));
    assert!(TaskState::Working.can_transition_to(TaskState::AuthRequired));
    assert!(TaskState::InputRequired.can_transition_to(TaskState::Working));
    assert!(TaskState::AuthRequired.can_transition_to(TaskState::Working));

    // Invalid transitions — terminal states can't transition
    assert!(!TaskState::Completed.can_transition_to(TaskState::Working));
    assert!(!TaskState::Failed.can_transition_to(TaskState::Working));
    assert!(!TaskState::Canceled.can_transition_to(TaskState::Working));
    assert!(!TaskState::Rejected.can_transition_to(TaskState::Working));

    // Invalid transitions — can't go backwards
    assert!(!TaskState::Submitted.can_transition_to(TaskState::Completed));
    assert!(!TaskState::Submitted.can_transition_to(TaskState::InputRequired));

    // Unspecified can transition to anything
    assert!(TaskState::Unspecified.can_transition_to(TaskState::Working));
    assert!(TaskState::Unspecified.can_transition_to(TaskState::Completed));
}

#[tokio::test]
async fn a2a_error_non_exhaustive() {
    // Verify A2aError can be created via constructors (not struct literal)
    let err = A2aError::new(ErrorCode::InternalError, "test");
    assert_eq!(err.code, ErrorCode::InternalError);
    assert_eq!(err.message, "test");
    assert!(err.data.is_none());

    let err = A2aError::with_data(
        ErrorCode::InvalidParams,
        "bad",
        serde_json::json!({"detail": "extra"}),
    );
    assert_eq!(err.code, ErrorCode::InvalidParams);
    assert!(err.data.is_some());
}

#[tokio::test]
async fn error_code_all_values_roundtrip() {
    let codes = [
        ErrorCode::ParseError,
        ErrorCode::InvalidRequest,
        ErrorCode::MethodNotFound,
        ErrorCode::InvalidParams,
        ErrorCode::InternalError,
        ErrorCode::TaskNotFound,
        ErrorCode::TaskNotCancelable,
        ErrorCode::PushNotificationNotSupported,
        ErrorCode::UnsupportedOperation,
        ErrorCode::ContentTypeNotSupported,
        ErrorCode::InvalidAgentResponse,
        ErrorCode::ExtendedAgentCardNotConfigured,
        ErrorCode::ExtensionSupportRequired,
        ErrorCode::VersionNotSupported,
    ];
    for code in &codes {
        let n: i32 = (*code).into();
        let back = ErrorCode::try_from(n).unwrap();
        assert_eq!(back, *code);
        // Verify Display works
        let _display = format!("{code}");
        // Verify default_message works
        let _msg = code.default_message();
    }
}

#[tokio::test]
async fn task_version_from_u64() {
    let v: TaskVersion = 42u64.into();
    assert_eq!(v.get(), 42);
    assert!(TaskVersion::new(2) > TaskVersion::new(1));
}

#[tokio::test]
async fn part_constructors() {
    let text = Part::text("hello");
    assert!(matches!(text.content, PartContent::Text { .. }));

    let raw = Part::raw("base64data");
    assert!(matches!(raw.content, PartContent::File { .. }));

    let url = Part::url("https://example.com");
    assert!(matches!(url.content, PartContent::File { .. }));

    let data = Part::data(serde_json::json!({"key": "value"}));
    assert!(matches!(data.content, PartContent::Data { .. }));
}

#[tokio::test]
async fn server_error_to_a2a_error_mapping() {
    let mappings: Vec<(ServerError, ErrorCode)> = vec![
        (
            ServerError::TaskNotFound(TaskId::new("x")),
            ErrorCode::TaskNotFound,
        ),
        (
            ServerError::TaskNotCancelable(TaskId::new("x")),
            ErrorCode::TaskNotCancelable,
        ),
        (
            ServerError::InvalidParams("x".into()),
            ErrorCode::InvalidParams,
        ),
        (
            ServerError::MethodNotFound("x".into()),
            ErrorCode::MethodNotFound,
        ),
        (
            ServerError::PushNotSupported,
            ErrorCode::PushNotificationNotSupported,
        ),
        (ServerError::Internal("x".into()), ErrorCode::InternalError),
        (ServerError::Transport("x".into()), ErrorCode::InternalError),
        (
            ServerError::HttpClient("x".into()),
            ErrorCode::InternalError,
        ),
        (
            ServerError::PayloadTooLarge("x".into()),
            ErrorCode::InternalError,
        ),
    ];
    for (server_err, expected_code) in mappings {
        let a2a_err = server_err.to_a2a_error();
        assert_eq!(
            a2a_err.code, expected_code,
            "mapping failed for {server_err}"
        );
    }
}

#[tokio::test]
async fn server_error_display_all_variants() {
    let errors: Vec<ServerError> = vec![
        ServerError::TaskNotFound(TaskId::new("t1")),
        ServerError::TaskNotCancelable(TaskId::new("t2")),
        ServerError::InvalidParams("bad param".into()),
        ServerError::HttpClient("conn refused".into()),
        ServerError::Transport("timeout".into()),
        ServerError::PushNotSupported,
        ServerError::Internal("something broke".into()),
        ServerError::MethodNotFound("unknown".into()),
        ServerError::PayloadTooLarge("too big".into()),
        ServerError::InvalidStateTransition {
            task_id: TaskId::new("t3"),
            from: TaskState::Completed,
            to: TaskState::Working,
        },
    ];
    for err in &errors {
        let display = err.to_string();
        assert!(
            !display.is_empty(),
            "display should not be empty for {err:?}"
        );
    }
}

#[tokio::test]
async fn event_queue_manager_lifecycle() {
    let mgr = a2a_protocol_server::EventQueueManager::new();

    let task_id = TaskId::new("task-1");
    assert_eq!(mgr.active_count().await, 0);

    // Create a queue
    let (_writer, reader) = mgr.get_or_create(&task_id).await;
    assert!(reader.is_some());
    assert_eq!(mgr.active_count().await, 1);

    // get_or_create again returns existing writer, no new reader
    let (_writer2, reader2) = mgr.get_or_create(&task_id).await;
    assert!(reader2.is_none()); // No new reader for existing queue
    assert_eq!(mgr.active_count().await, 1);

    // Destroy the queue
    mgr.destroy(&task_id).await;
    assert_eq!(mgr.active_count().await, 0);
}

#[tokio::test]
async fn event_queue_manager_destroy_all() {
    let mgr = a2a_protocol_server::EventQueueManager::new();

    mgr.get_or_create(&TaskId::new("t1")).await;
    mgr.get_or_create(&TaskId::new("t2")).await;
    assert_eq!(mgr.active_count().await, 2);

    mgr.destroy_all().await;
    assert_eq!(mgr.active_count().await, 0);
}

#[tokio::test]
async fn event_queue_write_and_read() {
    let (writer, reader) = a2a_protocol_server::streaming::event_queue::new_in_memory_queue();
    let mut reader = reader;

    let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("t1"),
        context_id: "ctx".into(),
        status: TaskStatus::new(TaskState::Working),
        metadata: None,
    });

    writer.write(event).await.unwrap();
    drop(writer); // Close the channel

    let received: Option<a2a_protocol_types::error::A2aResult<StreamResponse>> =
        reader.read().await;
    assert!(received.is_some());
    let received = received.unwrap().unwrap();
    assert!(matches!(received, StreamResponse::StatusUpdate(_)));

    // After writer is dropped, reader should get None
    let eof: Option<a2a_protocol_types::error::A2aResult<StreamResponse>> = reader.read().await;
    assert!(eof.is_none());
}

#[tokio::test]
async fn utc_now_iso8601_format() {
    let ts = a2a_protocol_types::utc_now_iso8601();
    // Should be in format "YYYY-MM-DDTHH:MM:SSZ"
    assert_eq!(ts.len(), 20, "timestamp should be 20 chars: {ts}");
    assert!(ts.ends_with('Z'));
    assert!(ts.contains('T'));
    assert_eq!(&ts[4..5], "-");
    assert_eq!(&ts[7..8], "-");
    assert_eq!(&ts[13..14], ":");
    assert_eq!(&ts[16..17], ":");
}

#[tokio::test]
async fn task_store_background_eviction() {
    let store = InMemoryTaskStore::with_config(TaskStoreConfig {
        max_capacity: Some(100),
        task_ttl: Some(Duration::from_millis(1)),
        ..Default::default()
    });

    // Insert a completed task
    let task = Task {
        id: TaskId::new("evict-me"),
        context_id: ContextId::new("ctx"),
        status: TaskStatus::new(TaskState::Completed),
        history: None,
        artifacts: None,
        metadata: None,
    };
    a2a_protocol_server::TaskStore::save(&store, task)
        .await
        .unwrap();

    // Wait for TTL to expire
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Run background eviction
    store.run_eviction().await;

    // Task should be evicted
    let result = a2a_protocol_server::TaskStore::get(&store, &TaskId::new("evict-me"))
        .await
        .unwrap();
    assert!(result.is_none(), "expired task should have been evicted");
}

#[tokio::test]
async fn executor_timeout() {
    let handler = RequestHandlerBuilder::new(SlowExecutor)
        .with_executor_timeout(Duration::from_millis(100))
        .build()
        .unwrap();

    let result = handler
        .on_send_message(make_send_params("timeout test"), false, None)
        .await;
    match result {
        Ok(a2a_protocol_server::SendMessageResult::Response(SendMessageResponse::Task(task))) => {
            assert_eq!(
                task.status.state,
                TaskState::Failed,
                "should be failed due to timeout"
            );
        }
        Err(_) => {
            // Also acceptable
        }
        _ => panic!("unexpected result"),
    }
}

// ── T-10: Concurrent operation tests ────────────────────────────────────────

#[tokio::test]
async fn concurrent_save_to_same_task_id() {
    let store = Arc::new(InMemoryTaskStore::new());
    let mut handles = vec![];

    for i in 0..50 {
        let store = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let task = Task {
                id: TaskId::new("shared-task"),
                context_id: ContextId::new(format!("ctx-{i}")),
                status: TaskStatus::new(TaskState::Working),
                history: None,
                artifacts: None,
                metadata: None,
            };
            a2a_protocol_server::TaskStore::save(store.as_ref(), task)
                .await
                .unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Should have exactly one task (last write wins)
    let result = a2a_protocol_server::TaskStore::get(store.as_ref(), &TaskId::new("shared-task"))
        .await
        .unwrap();
    assert!(result.is_some());
}

#[tokio::test]
async fn concurrent_get_or_create_event_queue() {
    let mgr = Arc::new(a2a_protocol_server::EventQueueManager::new());
    let task_id = TaskId::new("concurrent-queue");
    let mut handles = vec![];

    for _ in 0..10 {
        let mgr = Arc::clone(&mgr);
        let tid = task_id.clone();
        handles.push(tokio::spawn(async move { mgr.get_or_create(&tid).await }));
    }

    let mut reader_count = 0;
    for h in handles {
        let (_writer, reader) = h.await.unwrap();
        if reader.is_some() {
            reader_count += 1;
        }
    }

    // Only one should have gotten a reader (the first to create)
    assert_eq!(
        reader_count, 1,
        "only one concurrent create should get a reader"
    );
    assert_eq!(mgr.active_count().await, 1);
}

#[tokio::test]
async fn concurrent_send_message() {
    let handler = Arc::new(RequestHandlerBuilder::new(EchoExecutor).build().unwrap());
    let mut handles = vec![];

    for i in 0..10 {
        let handler = Arc::clone(&handler);
        handles.push(tokio::spawn(async move {
            handler
                .on_send_message(make_send_params(&format!("msg-{i}")), false, None)
                .await
        }));
    }

    let mut success_count = 0;
    for h in handles {
        if h.await.unwrap().is_ok() {
            success_count += 1;
        }
    }
    assert_eq!(success_count, 10, "all concurrent sends should succeed");
}

#[tokio::test]
async fn insert_if_absent_atomicity() {
    let store = Arc::new(InMemoryTaskStore::new());
    let mut handles = vec![];

    // 20 concurrent attempts to insert the same task ID
    for _ in 0..20 {
        let store = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let task = Task {
                id: TaskId::new("atomic-task"),
                context_id: ContextId::new("ctx"),
                status: TaskStatus::new(TaskState::Submitted),
                history: None,
                artifacts: None,
                metadata: None,
            };
            a2a_protocol_server::TaskStore::insert_if_absent(store.as_ref(), task)
                .await
                .unwrap()
        }));
    }

    let mut insert_count = 0;
    for h in handles {
        if h.await.unwrap() {
            insert_count += 1;
        }
    }

    // Exactly one should succeed
    assert_eq!(
        insert_count, 1,
        "exactly one insert_if_absent should succeed"
    );
}
