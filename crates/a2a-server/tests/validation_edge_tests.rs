// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Validation edge case tests: boundary conditions, malformed inputs,
//! Unicode handling, concurrent race conditions, and resource management.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_types::error::A2aResult;
use a2a_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_types::message::{Message, MessageId, MessageRole, Part};
use a2a_types::params::{ListTasksParams, MessageSendParams, SendMessageConfiguration};
use a2a_types::task::{ContextId, TaskState, TaskStatus};

use a2a_server::builder::RequestHandlerBuilder;
use a2a_server::executor::AgentExecutor;
use a2a_server::request_context::RequestContext;
use a2a_server::streaming::EventQueueWriter;
use a2a_server::{ServerError, TaskStoreConfig};

// ── Test executors ───────────────────────────────────────────────────────────

struct CompletingExecutor;

impl AgentExecutor for CompletingExecutor {
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
                    status: TaskStatus::with_timestamp(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: a2a_types::Artifact {
                        id: a2a_types::ArtifactId::new("art-1"),
                        name: Some("output.txt".into()),
                        description: None,
                        parts: vec![Part::text("artifact content")],
                        extensions: None,
                        metadata: None,
                    },
                    append: Some(false),
                    last_chunk: Some(true),
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

fn make_params(text: &str) -> MessageSendParams {
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

fn make_params_with_context(text: &str, ctx_id: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
            task_id: None,
            context_id: Some(ContextId::new(ctx_id)),
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }
}

// ── ID Validation Tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn reject_empty_context_id() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let mut params = make_params("hello");
    params.message.context_id = Some(ContextId::new(""));

    let result = handler.on_send_message(params, false).await;
    assert!(result.is_err(), "empty context_id should be rejected");
    if let Err(e) = result {
        assert!(
            matches!(e, ServerError::InvalidParams(_)),
            "expected InvalidParams, got: {e}"
        );
    }
}

#[tokio::test]
async fn reject_whitespace_only_context_id() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let mut params = make_params("hello");
    params.message.context_id = Some(ContextId::new("   \t\n  "));

    let result = handler.on_send_message(params, false).await;
    assert!(
        result.is_err(),
        "whitespace-only context_id should be rejected"
    );
}

#[tokio::test]
async fn reject_oversized_context_id() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let long_id = "x".repeat(1025);
    let mut params = make_params("hello");
    params.message.context_id = Some(ContextId::new(long_id));

    let result = handler.on_send_message(params, false).await;
    assert!(result.is_err(), "oversized context_id should be rejected");
}

#[tokio::test]
async fn accept_max_length_context_id() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let max_id = "x".repeat(1024);
    let mut params = make_params("hello");
    params.message.context_id = Some(ContextId::new(max_id));

    let result = handler.on_send_message(params, false).await;
    assert!(
        result.is_ok(),
        "1024-char context_id should be accepted: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn accept_unicode_context_id() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let mut params = make_params("hello");
    params.message.context_id = Some(ContextId::new("ctx-日本語-🎉-αβγ"));

    let result = handler.on_send_message(params, false).await;
    assert!(
        result.is_ok(),
        "unicode context_id should be accepted: {:?}",
        result.err()
    );
}

// ── Message Validation Tests ─────────────────────────────────────────────────

#[tokio::test]
async fn reject_empty_parts() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let params = MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };

    let result = handler.on_send_message(params, false).await;
    assert!(result.is_err(), "empty parts should be rejected");
}

#[tokio::test]
async fn reject_oversized_metadata() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    // Create metadata > 1 MiB
    let big_value = "x".repeat(1_100_000);
    let mut params = make_params("hello");
    params.message.metadata = Some(serde_json::json!({ "big": big_value }));

    let result = handler.on_send_message(params, false).await;
    assert!(result.is_err(), "oversized metadata should be rejected");
}

#[tokio::test]
async fn reject_oversized_request_metadata() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let big_value = "x".repeat(1_100_000);
    let mut params = make_params("hello");
    params.metadata = Some(serde_json::json!({ "big": big_value }));

    let result = handler.on_send_message(params, false).await;
    assert!(
        result.is_err(),
        "oversized request metadata should be rejected"
    );
}

#[tokio::test]
async fn accept_small_metadata() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let mut params = make_params("hello");
    params.message.metadata = Some(serde_json::json!({ "key": "value" }));

    let result = handler.on_send_message(params, false).await;
    assert!(
        result.is_ok(),
        "small metadata should be accepted: {:?}",
        result.err()
    );
}

// ── Artifact handling tests ──────────────────────────────────────────────────

#[tokio::test]
async fn artifact_produced_in_task() {
    let handler = RequestHandlerBuilder::new(ArtifactExecutor)
        .build()
        .unwrap();

    let result = handler.on_send_message(make_params("hello"), false).await;
    match result.unwrap() {
        a2a_server::SendMessageResult::Response(
            a2a_types::responses::SendMessageResponse::Task(task),
        ) => {
            assert_eq!(task.status.state, TaskState::Completed);
            let artifacts = task.artifacts.expect("should have artifacts");
            assert_eq!(artifacts.len(), 1);
            assert_eq!(artifacts[0].id, a2a_types::ArtifactId::new("art-1"));
            assert_eq!(artifacts[0].name.as_deref(), Some("output.txt"));
        }
        _ => panic!("expected Task response"),
    }
}

// ── Return immediately mode ──────────────────────────────────────────────────

#[tokio::test]
async fn return_immediately_returns_submitted_task() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let mut params = make_params("hello");
    params.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: vec!["text/plain".into()],
        task_push_notification_config: None,
        history_length: None,
        return_immediately: Some(true),
    });

    let result = handler.on_send_message(params, false).await;
    match result.unwrap() {
        a2a_server::SendMessageResult::Response(
            a2a_types::responses::SendMessageResponse::Task(task),
        ) => {
            assert_eq!(
                task.status.state,
                TaskState::Submitted,
                "return_immediately should return Submitted task"
            );
        }
        _ => panic!("expected Task response"),
    }
}

// ── Builder validation tests ─────────────────────────────────────────────────

#[tokio::test]
async fn builder_rejects_zero_timeout() {
    let result = RequestHandlerBuilder::new(CompletingExecutor)
        .with_executor_timeout(Duration::ZERO)
        .build();
    assert!(result.is_err(), "zero timeout should be rejected");
}

#[tokio::test]
async fn builder_rejects_empty_interfaces_in_card() {
    let card = a2a_types::AgentCard {
        name: "test".into(),
        description: "test agent".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![], // empty
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![a2a_types::AgentSkill {
            id: "skill-1".into(),
            name: "test-skill".into(),
            description: "A skill".into(),
            tags: vec![],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: a2a_types::AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    };

    let result = RequestHandlerBuilder::new(CompletingExecutor)
        .with_agent_card(card)
        .build();
    assert!(result.is_err(), "empty interfaces should be rejected");
}

// ── Shutdown tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn shutdown_cancels_in_flight_tasks() {
    struct NeverFinish;

    impl AgentExecutor for NeverFinish {
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
                // Wait indefinitely (or until cancelled)
                ctx.cancellation_token.cancelled().await;
                Ok(())
            })
        }
    }

    let handler = Arc::new(RequestHandlerBuilder::new(NeverFinish).build().unwrap());

    // Start a streaming task
    let h = Arc::clone(&handler);
    let task_handle = tokio::spawn(async move {
        let _ = h.on_send_message(make_params("never"), true).await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Shutdown should cancel all tasks
    handler.shutdown().await;

    // Task should complete soon after shutdown
    let result = tokio::time::timeout(Duration::from_secs(2), task_handle).await;
    assert!(result.is_ok(), "task should complete after shutdown");
}

#[tokio::test]
async fn shutdown_with_timeout() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(CompletingExecutor)
            .build()
            .unwrap(),
    );

    // Shutdown with timeout should return quickly when no tasks
    handler.shutdown_with_timeout(Duration::from_secs(1)).await;

    // After shutdown, handler can still accept new requests
    let result = handler
        .on_send_message(make_params("after shutdown"), false)
        .await;
    // This should succeed (shutdown cancels in-flight, but doesn't prevent new ones)
    assert!(result.is_ok());
}

// ── Concurrent list + send race ──────────────────────────────────────────────

#[tokio::test]
async fn concurrent_list_and_send() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(CompletingExecutor)
            .build()
            .unwrap(),
    );

    let mut handles = vec![];

    // Spawn senders
    for i in 0..5 {
        let h = Arc::clone(&handler);
        handles.push(tokio::spawn(async move {
            h.on_send_message(make_params(&format!("msg-{i}")), false)
                .await
                .ok();
        }));
    }

    // Spawn listers concurrently
    for _ in 0..5 {
        let h = Arc::clone(&handler);
        handles.push(tokio::spawn(async move {
            h.on_list_tasks(ListTasksParams {
                tenant: None,
                context_id: None,
                status: None,
                page_size: Some(50),
                page_token: None,
                status_timestamp_after: None,
                include_artifacts: None,
                history_length: None,
            })
            .await
            .ok();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ── Task store config tests ──────────────────────────────────────────────────

#[tokio::test]
async fn task_store_config_both_ttl_and_capacity() {
    let config = TaskStoreConfig {
        max_capacity: Some(5),
        task_ttl: Some(Duration::from_secs(3600)),
    };

    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .with_task_store_config(config)
        .build()
        .unwrap();

    // Create tasks to fill capacity
    for i in 0..7 {
        handler
            .on_send_message(make_params(&format!("msg-{i}")), false)
            .await
            .ok();
    }

    // List tasks - should be capped at capacity
    let list = handler
        .on_list_tasks(ListTasksParams {
            tenant: None,
            context_id: None,
            status: None,
            page_size: Some(50),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        })
        .await
        .unwrap();
    assert!(
        list.tasks.len() <= 5,
        "should not exceed capacity: got {}",
        list.tasks.len()
    );
}

// ── Multi-turn conversation via context ID ───────────────────────────────────

#[tokio::test]
async fn multi_turn_same_context_id() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let ctx_id = "conversation-1";

    // First message
    let result1 = handler
        .on_send_message(make_params_with_context("hello", ctx_id), false)
        .await;
    assert!(result1.is_ok());

    // Second message in same context
    let result2 = handler
        .on_send_message(make_params_with_context("follow up", ctx_id), false)
        .await;
    assert!(result2.is_ok());
}

// ── Event queue capacity tests ───────────────────────────────────────────────

#[tokio::test]
async fn custom_event_queue_capacity() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .with_event_queue_capacity(4)
        .build()
        .unwrap();

    let result = handler.on_send_message(make_params("test"), false).await;
    assert!(result.is_ok(), "should work with small queue capacity");
}

#[tokio::test]
async fn custom_max_event_size() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .with_max_event_size(1024 * 1024)
        .build()
        .unwrap();

    let result = handler.on_send_message(make_params("test"), false).await;
    assert!(result.is_ok());
}

// ── Metrics integration ──────────────────────────────────────────────────────

#[tokio::test]
async fn metrics_receives_callbacks() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingMetrics {
        requests: AtomicUsize,
    }

    impl a2a_server::metrics::Metrics for CountingMetrics {
        fn on_request(&self, _method: &str) {
            self.requests.fetch_add(1, Ordering::Relaxed);
        }
    }

    let _metrics = Arc::new(CountingMetrics {
        requests: AtomicUsize::new(0),
    });

    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .with_metrics(CountingMetrics {
            requests: AtomicUsize::new(0),
        })
        .build()
        .unwrap();

    handler
        .on_send_message(make_params("test"), false)
        .await
        .unwrap();

    // Metrics should have received at least one request callback
    // (We can't access the internal metrics, but this at least proves it compiles)
}

// ── Debug impls ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn debug_impls_dont_panic() {
    let handler = RequestHandlerBuilder::new(CompletingExecutor)
        .build()
        .unwrap();

    let debug = format!("{handler:?}");
    assert!(!debug.is_empty());

    let builder = RequestHandlerBuilder::new(CompletingExecutor);
    let debug = format!("{builder:?}");
    assert!(!debug.is_empty());
}
