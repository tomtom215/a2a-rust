// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Audit-driven tests covering P2 coverage gaps.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult, ErrorCode};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::push::{InMemoryPushConfigStore, PushConfigStore};
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::{EventQueueManager, EventQueueReader, EventQueueWriter};

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
                    artifact: Artifact::new("art-1", vec![Part::text("echo response")]),
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

/// An executor that waits on its cancellation token (for shutdown tests).
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
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            // Wait for cancellation instead of a fixed sleep, so shutdown
            // tests complete quickly.
            ctx.cancellation_token.cancelled().await;
            Ok(())
        })
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

/// Extracts the error from a `Result<SendMessageResult, ServerError>`,
/// panicking if it was `Ok`.
fn unwrap_send_err(
    result: Result<SendMessageResult, a2a_protocol_server::ServerError>,
) -> a2a_protocol_server::ServerError {
    match result {
        Err(e) => e,
        Ok(_) => panic!("expected error, got Ok(SendMessageResult)"),
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

// ── P2.1: Untested public APIs ──────────────────────────────────────────────

// 1. utc_now_iso8601

#[test]
fn utc_now_iso8601_returns_valid_iso_format() {
    let ts = a2a_protocol_types::utc_now_iso8601();

    // Should match YYYY-MM-DDTHH:MM:SSZ
    assert!(ts.ends_with('Z'), "timestamp should end with Z: {ts}");
    assert_eq!(ts.len(), 20, "expected 20-char ISO 8601 string: {ts}");

    // Verify the T separator is in the right place.
    assert_eq!(
        ts.as_bytes()[10],
        b'T',
        "expected T separator at position 10: {ts}"
    );

    // Verify dashes and colons are in the right places.
    assert_eq!(ts.as_bytes()[4], b'-', "expected dash at position 4: {ts}");
    assert_eq!(ts.as_bytes()[7], b'-', "expected dash at position 7: {ts}");
    assert_eq!(
        ts.as_bytes()[13],
        b':',
        "expected colon at position 13: {ts}"
    );
    assert_eq!(
        ts.as_bytes()[16],
        b':',
        "expected colon at position 16: {ts}"
    );

    // All other positions should be digits.
    for (i, ch) in ts.chars().enumerate() {
        if [4, 7, 10, 13, 16, 19].contains(&i) {
            continue; // separators
        }
        assert!(ch.is_ascii_digit(), "expected digit at position {i}: {ts}");
    }
}

// 2. A2aError named constructors

#[test]
fn a2a_error_internal_constructor() {
    let err = A2aError::internal("something broke");
    assert_eq!(err.code, ErrorCode::InternalError);
    assert_eq!(err.message, "something broke");
}

#[test]
fn a2a_error_invalid_params_constructor() {
    let err = A2aError::invalid_params("bad param");
    assert_eq!(err.code, ErrorCode::InvalidParams);
    assert_eq!(err.message, "bad param");
}

#[test]
fn a2a_error_unsupported_operation_constructor() {
    let err = A2aError::unsupported_operation("not supported");
    assert_eq!(err.code, ErrorCode::UnsupportedOperation);
    assert_eq!(err.message, "not supported");
}

#[test]
fn a2a_error_parse_error_constructor() {
    let err = A2aError::parse_error("bad json");
    assert_eq!(err.code, ErrorCode::ParseError);
    assert_eq!(err.message, "bad json");
}

#[test]
fn a2a_error_invalid_agent_response_constructor() {
    let err = A2aError::invalid_agent_response("bad response");
    assert_eq!(err.code, ErrorCode::InvalidAgentResponse);
    assert_eq!(err.message, "bad response");
}

#[test]
fn a2a_error_extended_card_not_configured_constructor() {
    let err = A2aError::extended_card_not_configured("no card");
    assert_eq!(err.code, ErrorCode::ExtendedAgentCardNotConfigured);
    assert_eq!(err.message, "no card");
}

// 3. Artifact::new()

#[test]
fn artifact_new_construction() {
    let artifact = Artifact::new(
        "test-artifact",
        vec![Part::text("hello"), Part::text("world")],
    );
    assert_eq!(artifact.id.0, "test-artifact");
    assert_eq!(artifact.parts.len(), 2);
    assert!(artifact.name.is_none());
    assert!(artifact.description.is_none());
    assert!(artifact.extensions.is_none());
    assert!(artifact.metadata.is_none());
}

#[test]
fn artifact_new_empty_parts() {
    let artifact = Artifact::new("empty", vec![]);
    assert_eq!(artifact.parts.len(), 0);
}

// 4. TaskPushNotificationConfig::new()

#[test]
fn push_config_new_construction() {
    let config = TaskPushNotificationConfig::new("task-42", "https://hooks.example.com/notify");
    assert_eq!(config.task_id, "task-42");
    assert_eq!(config.url, "https://hooks.example.com/notify");
    assert!(config.id.is_none());
    assert!(config.tenant.is_none());
    assert!(config.token.is_none());
    assert!(config.authentication.is_none());
}

// 5. RequestHandlerBuilder::with_executor_timeout() - timeout causes failure

#[tokio::test]
async fn executor_timeout_causes_failure() {
    let handler = RequestHandlerBuilder::new(SlowExecutor)
        .with_executor_timeout(Duration::from_millis(50))
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("slow"), false)
        .await;

    match result {
        Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => {
            assert_eq!(
                task.status.state,
                TaskState::Failed,
                "timed-out task should be Failed"
            );
        }
        Err(_) => {
            // Also acceptable — the timeout error may propagate.
        }
        _ => panic!("expected failed task or error from timeout"),
    }
}

// 6. RequestHandler::shutdown() - cancels in-flight tasks

#[tokio::test]
async fn shutdown_cancels_in_flight_tasks() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(SlowExecutor)
            .build()
            .expect("build handler"),
    );

    // Send a streaming message to create an in-flight task.
    let result = handler
        .on_send_message(make_send_params("slow-work"), true)
        .await
        .expect("send streaming message");

    // Verify we got a stream.
    assert!(matches!(result, SendMessageResult::Stream(_)));

    // Give the executor a moment to start and write the Working event.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Call shutdown — should cancel all in-flight tasks.
    handler.shutdown().await;

    // After shutdown, the event queues should be destroyed.
    // The reader should see EOF when drained.
    if let SendMessageResult::Stream(mut reader) = result {
        // Drain remaining events — stream should close.
        let mut event_count = 0;
        while let Some(_event) = reader.read().await {
            event_count += 1;
            if event_count > 100 {
                panic!("stream did not close after shutdown");
            }
        }
        // Stream closed successfully.
    }
}

// 7. RequestHandler::on_get_extended_agent_card() - configured and unconfigured

#[tokio::test]
async fn get_extended_agent_card_configured() {
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
    assert_eq!(card.supported_interfaces.len(), 1);
}

#[tokio::test]
async fn get_extended_agent_card_unconfigured() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let err = handler.on_get_extended_agent_card().await.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::Internal(_)),
        "expected Internal error, got {err:?}"
    );
}

// 8. CorsConfig::permissive()

#[test]
fn cors_config_permissive_returns_valid_config() {
    let cors = a2a_protocol_server::dispatch::CorsConfig::permissive();
    assert_eq!(cors.allow_origin, "*");
    assert!(!cors.allow_methods.is_empty());
    assert!(!cors.allow_headers.is_empty());
    assert!(cors.max_age_secs > 0);
}

#[test]
fn cors_config_new_with_specific_origin() {
    let cors = a2a_protocol_server::dispatch::CorsConfig::new("https://example.com");
    assert_eq!(cors.allow_origin, "https://example.com");
}

// 9. EventQueueManager::with_capacity() and ::destroy_all()

#[tokio::test]
async fn event_queue_manager_with_capacity() {
    let mgr = EventQueueManager::with_capacity(8);
    let task_id = TaskId::new("cap-test");

    let (writer, reader) = mgr.get_or_create(&task_id).await;
    assert!(reader.is_some(), "first call should return a reader");

    // Write an event to verify the queue works.
    writer
        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: task_id.clone(),
            context_id: ContextId::new("ctx"),
            status: TaskStatus::new(TaskState::Working),
            metadata: None,
        }))
        .await
        .expect("write event");

    let mut reader = reader.unwrap();
    let event = reader.read().await;
    assert!(event.is_some(), "should read back the written event");

    assert_eq!(mgr.active_count().await, 1);
}

#[tokio::test]
async fn event_queue_manager_destroy_all_clears() {
    let mgr = EventQueueManager::new();
    let t1 = TaskId::new("t1");
    let t2 = TaskId::new("t2");
    let t3 = TaskId::new("t3");

    mgr.get_or_create(&t1).await;
    mgr.get_or_create(&t2).await;
    mgr.get_or_create(&t3).await;

    assert_eq!(mgr.active_count().await, 3);

    mgr.destroy_all().await;

    assert_eq!(
        mgr.active_count().await,
        0,
        "destroy_all should clear all queues"
    );
}

// 10. RequestContext::with_stored_task(), ::with_metadata()

#[test]
fn request_context_with_stored_task() {
    let msg = make_message("test");
    let ctx = RequestContext::new(msg, TaskId::new("task-1"), "ctx-1".into());
    assert!(ctx.stored_task.is_none());

    let stored = a2a_protocol_types::task::Task {
        id: TaskId::new("old-task"),
        context_id: ContextId::new("ctx-1"),
        status: TaskStatus::new(TaskState::Completed),
        history: None,
        artifacts: None,
        metadata: None,
    };

    let ctx = ctx.with_stored_task(stored.clone());
    assert!(ctx.stored_task.is_some());
    assert_eq!(
        ctx.stored_task.as_ref().unwrap().id,
        TaskId::new("old-task")
    );
}

#[test]
fn request_context_with_metadata() {
    let msg = make_message("test");
    let ctx = RequestContext::new(msg, TaskId::new("task-1"), "ctx-1".into());
    assert!(ctx.metadata.is_none());

    let meta = serde_json::json!({"key": "value"});
    let ctx = ctx.with_metadata(meta.clone());
    assert_eq!(ctx.metadata, Some(meta));
}

// 11. CallContext::with_extensions()

#[test]
fn call_context_with_extensions() {
    let ctx = CallContext::new("test/method");
    assert!(ctx.extensions.is_empty());

    let ctx = ctx.with_extensions(vec!["urn:a2a:ext:foo".into(), "urn:a2a:ext:bar".into()]);
    assert_eq!(ctx.extensions.len(), 2);
    assert_eq!(ctx.extensions[0], "urn:a2a:ext:foo");
    assert_eq!(ctx.extensions[1], "urn:a2a:ext:bar");
}

#[test]
fn call_context_with_caller_identity() {
    let ctx = CallContext::new("SendMessage").with_caller_identity("user@example.com".into());
    assert_eq!(ctx.caller_identity.as_deref(), Some("user@example.com"));
}

// ── P2.2: Security-critical error paths ─────────────────────────────────────

// 12. ID length boundary: exactly 1024 chars passes, 1025 fails

#[tokio::test]
async fn context_id_exactly_max_length_passes() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let long_id = "x".repeat(1024);
    let mut msg = make_message("test");
    msg.context_id = Some(ContextId::new(&long_id));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let result = handler.on_send_message(params, false).await;
    assert!(
        result.is_ok(),
        "context_id of exactly 1024 chars should be accepted"
    );
}

#[tokio::test]
async fn context_id_over_max_length_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let long_id = "x".repeat(1025);
    let mut msg = make_message("test");
    msg.context_id = Some(ContextId::new(&long_id));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false).await);
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "context_id of 1025 chars should be rejected, got {err:?}"
    );
}

#[tokio::test]
async fn task_id_exactly_max_length_passes() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let long_id = "t".repeat(1024);
    let mut msg = make_message("test");
    msg.task_id = Some(TaskId::new(&long_id));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    // This will either succeed (if no existing task with that ID) or fail
    // for a reason other than length validation.
    let result = handler.on_send_message(params, false).await;
    // Should NOT fail with "exceeds maximum length"
    if let Err(ref e) = result {
        let msg = format!("{e}");
        assert!(
            !msg.contains("exceeds maximum length"),
            "task_id of exactly 1024 chars should not fail length check: {msg}"
        );
    }
}

#[tokio::test]
async fn task_id_over_max_length_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let long_id = "t".repeat(1025);
    let mut msg = make_message("test");
    msg.task_id = Some(TaskId::new(&long_id));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false).await);
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "task_id of 1025 chars should be rejected, got {err:?}"
    );
}

// 13. Empty string IDs rejected

#[tokio::test]
async fn empty_context_id_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let mut msg = make_message("test");
    msg.context_id = Some(ContextId::new(""));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false).await);
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "empty context_id should be rejected, got {err:?}"
    );
}

#[tokio::test]
async fn empty_task_id_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let mut msg = make_message("test");
    msg.task_id = Some(TaskId::new(""));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false).await);
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "empty task_id should be rejected, got {err:?}"
    );
}

// 14. Whitespace-only IDs rejected

#[tokio::test]
async fn whitespace_only_context_id_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let mut msg = make_message("test");
    msg.context_id = Some(ContextId::new("   \t\n  "));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false).await);
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "whitespace-only context_id should be rejected, got {err:?}"
    );
}

#[tokio::test]
async fn whitespace_only_task_id_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let mut msg = make_message("test");
    msg.task_id = Some(TaskId::new("   "));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false).await);
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "whitespace-only task_id should be rejected, got {err:?}"
    );
}

// ── P2.4: Edge case tests ───────────────────────────────────────────────────

// 15. Unicode/emoji in task IDs and messages

#[tokio::test]
async fn unicode_emoji_in_messages() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("Hello \u{1F600} world \u{1F310}"), false)
        .await
        .expect("send unicode message");

    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Response(Task)"),
    }
}

#[tokio::test]
async fn unicode_in_context_id() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let mut msg = make_message("test");
    msg.context_id = Some(ContextId::new("ctx-\u{1F680}-rocket"));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let result = handler.on_send_message(params, false).await;
    assert!(result.is_ok(), "unicode in context_id should be accepted");
}

// 16. Very large page_size clamped to 1000

#[tokio::test]
async fn large_page_size_clamped() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // Create a couple of tasks so we can list them.
    handler
        .on_send_message(make_send_params("one"), false)
        .await
        .expect("send first");
    handler
        .on_send_message(make_send_params("two"), false)
        .await
        .expect("send second");

    // Request u32::MAX page_size — should be clamped to 1000 internally.
    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: Some(u32::MAX),
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };

    let result = handler.on_list_tasks(params).await.expect("list tasks");
    // We only created 2 tasks, so we should get 2 back.
    // The important thing is that it does not crash or allocate huge memory.
    assert_eq!(result.tasks.len(), 2);
}

// 17. Duplicate task IDs rejected

#[tokio::test]
async fn duplicate_task_id_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // Create a task first.
    let result = handler
        .on_send_message(make_send_params("first"), false)
        .await
        .expect("send first");

    let task_id = match result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0.clone(),
        _ => panic!("expected task"),
    };

    // Try to create another task with the same task_id.
    let mut msg = make_message("duplicate");
    msg.task_id = Some(TaskId::new(&task_id));

    let params = MessageSendParams {
        tenant: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false).await);
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "duplicate task_id should be rejected, got {err:?}"
    );
}

// 18. Push config per-task limit

#[tokio::test]
async fn push_config_per_task_limit() {
    let store = InMemoryPushConfigStore::new();

    // Add 100 configs with unique IDs for the same task — all should succeed.
    for i in 0..100 {
        let mut config = TaskPushNotificationConfig::new("task-limit", "https://hooks.example.com");
        config.id = Some(format!("cfg-{i}"));
        let result = store.set(config).await;
        assert!(result.is_ok(), "config {i} should succeed, got {result:?}");
    }

    // 101st should fail.
    let mut config = TaskPushNotificationConfig::new("task-limit", "https://hooks.example.com");
    config.id = Some("cfg-overflow".into());
    let err = store.set(config).await.unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::InvalidParams,
        "101st config should be rejected with InvalidParams"
    );
    assert!(
        err.message.contains("limit exceeded"),
        "error message should mention limit: {}",
        err.message
    );
}

// 19. Event size limit

#[tokio::test]
async fn oversized_event_rejected() {
    use a2a_protocol_server::streaming::event_queue::new_in_memory_queue_with_options;

    // Create a queue with a very small max event size (128 bytes).
    let (writer, _reader) = new_in_memory_queue_with_options(8, 128);

    // Create an event with a large payload that exceeds 128 bytes.
    let big_text = "x".repeat(500);
    let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("big-task"),
        context_id: ContextId::new("big-ctx"),
        status: TaskStatus {
            state: TaskState::Working,
            message: Some(Message {
                id: MessageId::new("m1"),
                role: MessageRole::Agent,
                parts: vec![Part::text(&big_text)],
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            }),
            timestamp: None,
        },
        metadata: None,
    });

    let err = writer.write(event).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InternalError);
    assert!(
        err.message.contains("exceeds maximum"),
        "error should mention size exceeded: {}",
        err.message
    );
}

// 20. Builder validation: zero timeout rejected, empty interfaces rejected

#[test]
fn builder_rejects_zero_timeout() {
    let result = RequestHandlerBuilder::new(EchoExecutor)
        .with_executor_timeout(Duration::ZERO)
        .build();

    assert!(result.is_err(), "zero timeout should be rejected");
    let err = result.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "expected InvalidParams for zero timeout, got {err:?}"
    );
}

#[test]
fn builder_rejects_empty_interfaces() {
    let mut card = minimal_agent_card();
    card.supported_interfaces.clear();

    let result = RequestHandlerBuilder::new(EchoExecutor)
        .with_agent_card(card)
        .build();

    assert!(result.is_err(), "empty interfaces should be rejected");
    let err = result.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "expected InvalidParams for empty interfaces, got {err:?}"
    );
}

#[test]
fn builder_accepts_valid_timeout() {
    let result = RequestHandlerBuilder::new(EchoExecutor)
        .with_executor_timeout(Duration::from_secs(30))
        .build();

    assert!(result.is_ok(), "valid timeout should be accepted");
}

// ── P2.5: Integration test gaps ─────────────────────────────────────────────

// 21. Full handler lifecycle: send -> get -> list -> cancel flow

#[tokio::test]
async fn full_handler_lifecycle_send_get_list_cancel() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(EchoExecutor)
            .build()
            .expect("build handler"),
    );

    // Step 1: Send a message and get a completed task.
    let send_result = handler
        .on_send_message(make_send_params("lifecycle test"), false)
        .await
        .expect("send message");

    let task = match send_result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t,
        _ => panic!("expected Response(Task)"),
    };
    assert_eq!(task.status.state, TaskState::Completed);
    let task_id = task.id.clone();

    // Step 2: Get the task by ID.
    let fetched = handler
        .on_get_task(a2a_protocol_types::params::TaskQueryParams {
            tenant: None,
            id: task_id.0.clone(),
            history_length: None,
        })
        .await
        .expect("get task");
    assert_eq!(fetched.id, task_id);
    assert_eq!(fetched.status.state, TaskState::Completed);

    // Step 3: List tasks — should include our task.
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
    assert!(
        list.tasks.iter().any(|t| t.id == task_id),
        "listed tasks should include the created task"
    );

    // Step 4: Cancel a completed task — should fail with TaskNotCancelable.
    let cancel_err = handler
        .on_cancel_task(a2a_protocol_types::params::CancelTaskParams {
            tenant: None,
            id: task_id.0.clone(),
            metadata: None,
        })
        .await
        .unwrap_err();
    assert!(
        matches!(
            cancel_err,
            a2a_protocol_server::ServerError::TaskNotCancelable(_)
        ),
        "canceling a completed task should fail with TaskNotCancelable, got {cancel_err:?}"
    );
}

#[tokio::test]
async fn full_handler_lifecycle_with_streaming() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // Send streaming message.
    let result = handler
        .on_send_message(make_send_params("stream lifecycle"), true)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected stream"),
    };

    // Collect all events.
    let mut states = vec![];
    let mut artifact_count = 0;
    while let Some(event) = reader.read().await {
        match event.expect("event ok") {
            StreamResponse::StatusUpdate(u) => states.push(u.status.state),
            StreamResponse::ArtifactUpdate(_) => artifact_count += 1,
            _ => {}
        }
    }

    // Verify event order: Working, then Completed.
    assert_eq!(
        states,
        vec![TaskState::Working, TaskState::Completed],
        "expected Working -> Completed state sequence"
    );
    assert_eq!(artifact_count, 1, "expected exactly one artifact event");

    // After the stream is exhausted, list tasks to verify the task was stored.
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
    assert!(
        !list.tasks.is_empty(),
        "should have at least one task after streaming"
    );
}

#[tokio::test]
async fn full_handler_lifecycle_failing_executor() {
    let handler = RequestHandlerBuilder::new(FailingExecutor)
        .build()
        .expect("build handler");

    // Non-streaming: should produce a failed task.
    let result = handler
        .on_send_message(make_send_params("fail"), false)
        .await;

    match result {
        Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => {
            assert_eq!(task.status.state, TaskState::Failed);
        }
        Err(_) => {
            // Acceptable — error may propagate directly.
        }
        _ => panic!("expected failed task or error"),
    }

    // Streaming: should produce a Failed status event.
    let result = handler
        .on_send_message(make_send_params("fail-stream"), true)
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
    assert!(saw_failed, "expected a Failed status update in the stream");
}
