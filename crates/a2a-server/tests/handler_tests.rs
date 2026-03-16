// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Integration tests for [`RequestHandler`].

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{
    CancelTaskParams, DeletePushConfigParams, GetPushConfigParams, ListTasksParams,
    MessageSendParams, TaskIdParams, TaskQueryParams,
};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::push::PushSender;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::{EventQueueReader, EventQueueWriter};

// ── Test executor implementations ────────────────────────────────────────────

/// An executor that immediately completes with a status update.
struct EchoExecutor;

impl AgentExecutor for EchoExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Write a working status update.
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // Write an artifact.
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("art-1", vec![Part::text("echo response")]),
                    append: None,
                    last_chunk: None,
                    metadata: None,
                }))
                .await?;

            // Write completed status.
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

/// An executor that fails during execution.
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

/// An executor that supports cancellation.
struct CancelableExecutor;

impl AgentExecutor for CancelableExecutor {
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
            // Simulate long-running work by sleeping (won't actually block tests
            // because we test cancel on already-stored tasks).
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(())
        })
    }

    fn cancel<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

/// A no-op push sender for testing.
struct MockPushSender;

impl PushSender for MockPushSender {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn make_message(text: &str) -> Message {
    Message {
        id: MessageId::new("msg-1"),
        role: MessageRole::User,
        parts: vec![Part::text(text)],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: make_message(text),
        configuration: None,
        metadata: None,
    }
}

fn minimal_agent_card() -> AgentCard {
    AgentCard {
        name: "Test Agent".into(),
        description: "A test agent".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "https://agent.example.com/rpc".into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "echo".into(),
            name: "Echo".into(),
            description: "Echoes input".into(),
            tags: vec!["echo".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

// ── Handler tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn send_message_returns_completed_task() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("hello"), false)
        .await
        .expect("send message");

    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed);
            assert!(task.artifacts.is_some());
            let artifacts = task.artifacts.unwrap();
            assert_eq!(artifacts.len(), 1);
        }
        _ => panic!("expected Response(Task)"),
    }
}

#[tokio::test]
async fn send_message_streaming_returns_reader() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("hello"), true)
        .await
        .expect("send streaming message");

    match result {
        SendMessageResult::Stream(mut reader) => {
            let mut events = vec![];
            while let Some(event) = reader.read().await {
                events.push(event.expect("event should be ok"));
            }
            // Should have: Working status, ArtifactUpdate, Completed status.
            assert!(
                events.len() >= 3,
                "expected at least 3 events, got {}",
                events.len()
            );

            // First event should be Working status.
            assert!(
                matches!(&events[0], StreamResponse::StatusUpdate(u) if u.status.state == TaskState::Working)
            );
            // Second should be an artifact update.
            assert!(matches!(&events[1], StreamResponse::ArtifactUpdate(_)));
            // Third (or later) should be Completed status.
            assert!(
                matches!(&events[2], StreamResponse::StatusUpdate(u) if u.status.state == TaskState::Completed)
            );
        }
        _ => panic!("expected Stream, got unexpected variant"),
    }
}

#[tokio::test]
async fn send_message_executor_failure_results_in_failed_task() {
    let handler = RequestHandlerBuilder::new(FailingExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("boom"), false)
        .await;

    // The handler catches the executor error and writes a Failed status.
    // The collect_events loop should see the Failed status and return it.
    match result {
        Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => {
            assert_eq!(task.status.state, TaskState::Failed);
        }
        Err(_) => {
            // Also acceptable — the error might propagate.
        }
        _ => panic!("expected failed task or error, got unexpected variant"),
    }
}

#[tokio::test]
async fn get_task_returns_stored_task() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // First send a message to create a task.
    let result = handler
        .on_send_message(make_send_params("hello"), false)
        .await
        .expect("send message");

    let task_id = match result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.clone(),
        _ => panic!("expected task, got unexpected variant"),
    };

    // Now get the task.
    let params = TaskQueryParams {
        tenant: None,
        id: task_id.0.clone(),
        history_length: None,
    };
    let task = handler.on_get_task(params).await.expect("get task");
    assert_eq!(task.id, task_id);
    assert_eq!(task.status.state, TaskState::Completed);
}

#[tokio::test]
async fn get_task_not_found() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let params = TaskQueryParams {
        tenant: None,
        id: "nonexistent".into(),
        history_length: None,
    };
    let err = handler.on_get_task(params).await.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::TaskNotFound(_)),
        "expected TaskNotFound, got {err:?}"
    );
}

#[tokio::test]
async fn list_tasks_returns_created_tasks() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // Create two tasks.
    handler
        .on_send_message(make_send_params("one"), false)
        .await
        .expect("send first");
    handler
        .on_send_message(make_send_params("two"), false)
        .await
        .expect("send second");

    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = handler.on_list_tasks(params).await.expect("list tasks");
    assert_eq!(result.tasks.len(), 2);
}

#[tokio::test]
async fn cancel_task_on_working_task() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(CancelableExecutor)
            .build()
            .expect("build handler"),
    );

    // Send a streaming message to get a task in-progress.
    let result = handler
        .on_send_message(make_send_params("work"), true)
        .await
        .expect("send message");

    // Get the task ID from the store (list all tasks).
    let list = handler
        .on_list_tasks(ListTasksParams {
            tenant: None,
            context_id: None,
            status: None,
            page_size: None,
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        })
        .await
        .expect("list tasks");

    assert!(!list.tasks.is_empty());
    let task = &list.tasks[0];

    // Cancel the task (it should be in Pending or Working state).
    let cancel_params = CancelTaskParams {
        tenant: None,
        id: task.id.0.clone(),
        metadata: None,
    };
    let canceled = handler.on_cancel_task(cancel_params).await.expect("cancel");
    assert_eq!(canceled.status.state, TaskState::Canceled);

    // Drop the stream reader to clean up.
    drop(result);
}

#[tokio::test]
async fn cancel_terminal_task_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // Create and complete a task.
    let result = handler
        .on_send_message(make_send_params("done"), false)
        .await
        .expect("send message");

    let task_id = match result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0.clone(),
        _ => panic!("expected task, got unexpected variant"),
    };

    // Try to cancel the completed task.
    let cancel_params = CancelTaskParams {
        tenant: None,
        id: task_id,
        metadata: None,
    };
    let err = handler.on_cancel_task(cancel_params).await.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::TaskNotCancelable(_)),
        "expected TaskNotCancelable, got {err:?}"
    );
}

#[tokio::test]
async fn cancel_nonexistent_task_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let cancel_params = CancelTaskParams {
        tenant: None,
        id: "does-not-exist".into(),
        metadata: None,
    };
    let err = handler.on_cancel_task(cancel_params).await.unwrap_err();
    assert!(matches!(err, a2a_protocol_server::ServerError::TaskNotFound(_)));
}

// ── Push notification config tests ──────────────────────────────────────────

#[tokio::test]
async fn push_config_crud_lifecycle() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .with_push_sender(MockPushSender)
        .build()
        .expect("build handler");

    // Create push config.
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/webhook");
    let created = handler
        .on_set_push_config(config)
        .await
        .expect("set push config");
    assert!(created.id.is_some());
    let config_id = created.id.clone().unwrap();

    // Get push config.
    let get_params = GetPushConfigParams {
        tenant: None,
        task_id: "task-1".into(),
        id: config_id.clone(),
    };
    let fetched = handler
        .on_get_push_config(get_params)
        .await
        .expect("get push config");
    assert_eq!(fetched.url, "https://example.com/webhook");

    // List push configs.
    let configs = handler
        .on_list_push_configs("task-1")
        .await
        .expect("list push configs");
    assert_eq!(configs.len(), 1);

    // Delete push config.
    let delete_params = DeletePushConfigParams {
        tenant: None,
        task_id: "task-1".into(),
        id: config_id,
    };
    handler
        .on_delete_push_config(delete_params)
        .await
        .expect("delete push config");

    // Verify deleted.
    let configs = handler
        .on_list_push_configs("task-1")
        .await
        .expect("list push configs after delete");
    assert!(configs.is_empty());
}

#[tokio::test]
async fn push_config_not_supported_without_sender() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/webhook");
    let err = handler.on_set_push_config(config).await.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::PushNotSupported),
        "expected PushNotSupported, got {err:?}"
    );
}

#[tokio::test]
async fn get_push_config_not_found() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .with_push_sender(MockPushSender)
        .build()
        .expect("build handler");

    let params = GetPushConfigParams {
        tenant: None,
        task_id: "task-1".into(),
        id: "nonexistent".into(),
    };
    let err = handler.on_get_push_config(params).await.unwrap_err();
    assert!(matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)));
}

// ── Extended agent card tests ───────────────────────────────────────────────

#[tokio::test]
async fn get_extended_agent_card_returns_card() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .with_agent_card(minimal_agent_card())
        .build()
        .expect("build handler");

    let card = handler
        .on_get_extended_agent_card()
        .await
        .expect("get agent card");
    assert_eq!(card.name, "Test Agent");
    assert_eq!(card.version, "1.0.0");
}

#[tokio::test]
async fn get_extended_agent_card_not_configured() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let err = handler.on_get_extended_agent_card().await.unwrap_err();
    assert!(matches!(err, a2a_protocol_server::ServerError::Internal(_)));
}

// ── Event queue streaming tests ─────────────────────────────────────────────

#[tokio::test]
async fn streaming_events_arrive_in_order() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("test"), true)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected stream"),
    };

    let mut states = vec![];
    while let Some(event) = reader.read().await {
        if let Ok(StreamResponse::StatusUpdate(u)) = event {
            states.push(u.status.state);
        }
    }

    assert_eq!(states, vec![TaskState::Working, TaskState::Completed]);
}

#[tokio::test]
async fn streaming_failure_produces_failed_event() {
    let handler = RequestHandlerBuilder::new(FailingExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("boom"), true)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected stream"),
    };

    let mut saw_failed = false;
    while let Some(event) = reader.read().await {
        if let Ok(StreamResponse::StatusUpdate(u)) = event {
            if u.status.state == TaskState::Failed {
                saw_failed = true;
            }
        }
    }

    assert!(saw_failed, "expected a Failed status update in stream");
}

// ── Builder tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn builder_defaults_work() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build with defaults");

    // Verify handler works with default in-memory stores.
    let result = handler
        .on_send_message(make_send_params("test"), false)
        .await
        .expect("send message");
    assert!(matches!(result, SendMessageResult::Response(_)));
}

#[tokio::test]
async fn builder_with_agent_card() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .with_agent_card(minimal_agent_card())
        .with_push_sender(MockPushSender)
        .build()
        .expect("build with card + push");

    let card = handler
        .on_get_extended_agent_card()
        .await
        .expect("get card");
    assert_eq!(card.name, "Test Agent");
}

// ── Resubscribe tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn resubscribe_nonexistent_task_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let params = TaskIdParams {
        tenant: None,
        id: "nonexistent".into(),
    };
    let err = handler.on_resubscribe(params).await.unwrap_err();
    assert!(matches!(err, a2a_protocol_server::ServerError::TaskNotFound(_)));
}

// ── Phase 7 tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn return_immediately_returns_pending_task() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let params = MessageSendParams {
        tenant: None,
        message: make_message("hello"),
        configuration: Some(a2a_protocol_types::params::SendMessageConfiguration {
            accepted_output_modes: vec!["text/plain".into()],
            task_push_notification_config: None,
            history_length: None,
            return_immediately: Some(true),
        }),
        metadata: None,
    };

    let result = handler
        .on_send_message(params, false)
        .await
        .expect("send message");

    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(
                task.status.state,
                TaskState::Submitted,
                "return_immediately should return Submitted task"
            );
        }
        _ => panic!("expected Response(Task)"),
    }
}

#[tokio::test]
async fn task_continuation_same_context_finds_stored_task() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // First message creates a task with a specific context_id.
    let mut msg1 = make_message("first");
    msg1.context_id = Some(a2a_protocol_types::task::ContextId::new("ctx-continuation"));

    let result1 = handler
        .on_send_message(
            MessageSendParams {
                tenant: None,
                message: msg1,
                configuration: None,
                metadata: None,
            },
            false,
        )
        .await
        .expect("first send");

    let task_id_1 = match result1 {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.clone(),
        _ => panic!("expected task"),
    };

    // Second message with same context_id should create a NEW task but have
    // stored_task set. We verify indirectly by checking two tasks exist.
    let mut msg2 = make_message("second");
    msg2.context_id = Some(a2a_protocol_types::task::ContextId::new("ctx-continuation"));

    let result2 = handler
        .on_send_message(
            MessageSendParams {
                tenant: None,
                message: msg2,
                configuration: None,
                metadata: None,
            },
            false,
        )
        .await
        .expect("second send");

    let task_id_2 = match result2 {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.clone(),
        _ => panic!("expected task"),
    };

    // Two different tasks should be created.
    assert_ne!(task_id_1, task_id_2, "second send should create a new task");

    // Both tasks should be in the store.
    let list = handler
        .on_list_tasks(ListTasksParams {
            tenant: None,
            context_id: Some("ctx-continuation".into()),
            status: None,
            page_size: None,
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        })
        .await
        .expect("list tasks");
    assert!(
        list.tasks.len() >= 2,
        "should have at least 2 tasks for the context"
    );
}

#[tokio::test]
async fn context_task_mismatch_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // Create a task with a specific context.
    let mut msg1 = make_message("first");
    msg1.context_id = Some(a2a_protocol_types::task::ContextId::new("ctx-mismatch"));

    handler
        .on_send_message(
            MessageSendParams {
                tenant: None,
                message: msg1,
                configuration: None,
                metadata: None,
            },
            false,
        )
        .await
        .expect("first send");

    // Second message with same context but WRONG task_id should be rejected.
    let mut msg2 = make_message("second");
    msg2.context_id = Some(a2a_protocol_types::task::ContextId::new("ctx-mismatch"));
    msg2.task_id = Some(TaskId::new("wrong-task-id"));

    let result = handler
        .on_send_message(
            MessageSendParams {
                tenant: None,
                message: msg2,
                configuration: None,
                metadata: None,
            },
            false,
        )
        .await;
    let err = result.err().expect("expected error for task_id mismatch");

    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "expected InvalidParams for task_id mismatch, got {err:?}"
    );
}

/// Interceptor that rejects all requests.
struct RejectInterceptor;

impl a2a_protocol_server::interceptor::ServerInterceptor for RejectInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a a2a_protocol_server::call_context::CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async {
            Err(A2aError::new(
                a2a_protocol_types::error::ErrorCode::UnsupportedOperation,
                "rejected by interceptor",
            ))
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a a2a_protocol_server::call_context::CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn interceptor_rejection_stops_processing() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .with_interceptor(RejectInterceptor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("hello"), false)
        .await;
    let err = result.err().expect("expected error from interceptor");

    // The error should be a Protocol error from the interceptor.
    assert!(
        matches!(err, a2a_protocol_server::ServerError::Protocol(_)),
        "expected Protocol error from interceptor, got {err:?}"
    );
}

// ── Error conversion tests ──────────────────────────────────────────────────

#[test]
fn server_error_to_a2a_error_mapping() {
    use a2a_protocol_server::ServerError;
    use a2a_protocol_types::error::ErrorCode;

    let cases = vec![
        (
            ServerError::TaskNotFound(TaskId::new("t1")),
            ErrorCode::TaskNotFound,
        ),
        (
            ServerError::TaskNotCancelable(TaskId::new("t1")),
            ErrorCode::TaskNotCancelable,
        ),
        (
            ServerError::InvalidParams("bad".into()),
            ErrorCode::InvalidParams,
        ),
        (
            ServerError::PushNotSupported,
            ErrorCode::PushNotificationNotSupported,
        ),
        (
            ServerError::MethodNotFound("Foo".into()),
            ErrorCode::MethodNotFound,
        ),
        (
            ServerError::Internal("oops".into()),
            ErrorCode::InternalError,
        ),
    ];

    for (server_err, expected_code) in cases {
        let a2a_err = server_err.to_a2a_error();
        assert_eq!(
            a2a_err.code, expected_code,
            "mapping mismatch for {server_err}"
        );
    }
}
