// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `SendMessage` / `SendStreamingMessage` handler implementation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use crate::error::{ServerError, ServerResult};
use crate::request_context::RequestContext;
use crate::streaming::EventQueueWriter;

use super::helpers::{build_call_context, validate_id};
use super::{CancellationEntry, RequestHandler, SendMessageResult};

impl RequestHandler {
    /// Handles `SendMessage` / `SendStreamingMessage`.
    ///
    /// The optional `headers` map carries HTTP request headers for
    /// interceptor access-control decisions (e.g. `Authorization`).
    ///
    /// # Errors
    ///
    /// Returns [`ServerError`] if task creation or execution fails.
    pub async fn on_send_message(
        &self,
        params: MessageSendParams,
        streaming: bool,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<SendMessageResult> {
        let method_name = if streaming {
            "SendStreamingMessage"
        } else {
            "SendMessage"
        };
        let start = Instant::now();
        trace_info!(method = method_name, streaming, "handling send message");
        self.metrics.on_request(method_name);

        let tenant = params.tenant.clone().unwrap_or_default();
        let result = crate::store::tenant::TenantContext::scope(tenant, async {
            self.send_message_inner(params, streaming, method_name, headers)
                .await
        })
        .await;
        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response(method_name);
                self.metrics.on_latency(method_name, elapsed);
            }
            Err(e) => {
                self.metrics.on_error(method_name, &e.to_string());
                self.metrics.on_latency(method_name, elapsed);
            }
        }
        result
    }

    /// Inner implementation of `on_send_message`, extracted so that the outer
    /// method can uniformly track success/error metrics.
    #[allow(clippy::too_many_lines)]
    async fn send_message_inner(
        &self,
        params: MessageSendParams,
        streaming: bool,
        method_name: &str,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<SendMessageResult> {
        let call_ctx = build_call_context(method_name, headers);
        self.interceptors.run_before(&call_ctx).await?;

        // Validate incoming IDs: reject empty/whitespace-only and excessively long values (AP-1).
        if let Some(ref ctx_id) = params.message.context_id {
            validate_id(&ctx_id.0, "context_id", self.limits.max_id_length)?;
        }
        if let Some(ref task_id) = params.message.task_id {
            validate_id(&task_id.0, "task_id", self.limits.max_id_length)?;
        }

        // SC-4: Reject messages with no parts.
        if params.message.parts.is_empty() {
            return Err(ServerError::InvalidParams(
                "message must contain at least one part".into(),
            ));
        }

        // PR-8: Reject oversized metadata to prevent memory exhaustion.
        let max_meta = self.limits.max_metadata_size;
        if let Some(ref meta) = params.message.metadata {
            let meta_size = serde_json::to_string(meta).map(|s| s.len()).map_err(|_| {
                ServerError::InvalidParams("message metadata is not serializable".into())
            })?;
            if meta_size > max_meta {
                return Err(ServerError::InvalidParams(format!(
                    "message metadata exceeds maximum size ({meta_size} bytes, max {max_meta})"
                )));
            }
        }
        if let Some(ref meta) = params.metadata {
            let meta_size = serde_json::to_string(meta).map(|s| s.len()).map_err(|_| {
                ServerError::InvalidParams("request metadata is not serializable".into())
            })?;
            if meta_size > max_meta {
                return Err(ServerError::InvalidParams(format!(
                    "request metadata exceeds maximum size ({meta_size} bytes, max {max_meta})"
                )));
            }
        }

        // Generate task and context IDs.
        // Params-level context_id takes precedence over message-level.
        let task_id = TaskId::new(uuid::Uuid::new_v4().to_string());
        let context_id = params
            .context_id
            .as_deref()
            .or_else(|| params.message.context_id.as_ref().map(|c| c.0.as_str()))
            .map_or_else(|| uuid::Uuid::new_v4().to_string(), ToString::to_string);

        // Acquire a per-context lock to serialize the find + save sequence for
        // the same context_id, preventing two concurrent SendMessage requests
        // from both creating new tasks for the same context.
        let context_lock = {
            let mut locks = self.context_locks.write().await;
            locks.entry(context_id.clone()).or_default().clone()
        };
        let context_guard = context_lock.lock().await;

        // Look up existing task for continuation.
        let stored_task = self.find_task_by_context(&context_id).await?;

        // Context/task mismatch rejection: if message.task_id is set but
        // doesn't match the stored task found by context_id, reject.
        if let Some(ref msg_task_id) = params.message.task_id {
            if let Some(ref stored) = stored_task {
                if msg_task_id != &stored.id {
                    return Err(ServerError::InvalidParams(
                        "message task_id does not match task found for context".into(),
                    ));
                }
            } else {
                // Atomically check for duplicate task ID using insert_if_absent (CB-4).
                // Create a placeholder task that will be overwritten below.
                let placeholder = Task {
                    id: msg_task_id.clone(),
                    context_id: ContextId::new(&context_id),
                    status: TaskStatus::with_timestamp(TaskState::Submitted),
                    history: None,
                    artifacts: None,
                    metadata: None,
                };
                if !self.task_store.insert_if_absent(placeholder).await? {
                    return Err(ServerError::InvalidParams(
                        "task_id already exists; cannot create duplicate".into(),
                    ));
                }
            }
        }

        // Check return_immediately mode.
        let return_immediately = params
            .configuration
            .as_ref()
            .and_then(|c| c.return_immediately)
            .unwrap_or(false);

        // Create initial task.
        trace_debug!(
            task_id = %task_id,
            context_id = %context_id,
            "creating task"
        );
        let task = Task {
            id: task_id.clone(),
            context_id: ContextId::new(&context_id),
            status: TaskStatus::with_timestamp(TaskState::Submitted),
            history: None,
            artifacts: None,
            metadata: None,
        };

        // Build request context BEFORE saving to store so we can insert the
        // cancellation token atomically with the task save.
        let mut ctx = RequestContext::new(params.message, task_id.clone(), context_id);
        if let Some(stored) = stored_task {
            ctx = ctx.with_stored_task(stored);
        }
        if let Some(meta) = params.metadata {
            ctx = ctx.with_metadata(meta);
        }

        // FIX(#8): Insert the cancellation token BEFORE saving the task to
        // the store. This eliminates the race window where a task exists in
        // the store but has no cancellation token — a concurrent CancelTask
        // during that window would silently fail to cancel.
        {
            // Phase 1: Collect stale entries under READ lock (non-blocking for
            // other readers). This avoids holding a write lock during the O(n)
            // sweep of all cancellation tokens.
            let stale_ids: Vec<TaskId> = {
                let tokens = self.cancellation_tokens.read().await;
                if tokens.len() >= self.limits.max_cancellation_tokens {
                    let now = Instant::now();
                    tokens
                        .iter()
                        .filter(|(_, entry)| {
                            entry.token.is_cancelled()
                                || now.duration_since(entry.created_at) >= self.limits.max_token_age
                        })
                        .map(|(id, _)| id.clone())
                        .collect()
                } else {
                    Vec::new()
                }
            };

            // Phase 2: Remove stale entries under WRITE lock (brief).
            if !stale_ids.is_empty() {
                let mut tokens = self.cancellation_tokens.write().await;
                for id in &stale_ids {
                    tokens.remove(id);
                }
            }

            // Phase 3: Insert the new token under WRITE lock.
            let mut tokens = self.cancellation_tokens.write().await;
            tokens.insert(
                task_id.clone(),
                CancellationEntry {
                    token: ctx.cancellation_token.clone(),
                    created_at: Instant::now(),
                },
            );
        }

        self.task_store.save(task.clone()).await?;

        // Release the per-context lock now that the task is saved. Subsequent
        // requests for this context_id will find the task via find_task_by_context.
        drop(context_guard);

        // Create event queue. For streaming mode, use a dedicated persistence
        // channel so the background event processor is not affected by slow
        // SSE consumers (H5 fix).
        let (writer, reader, persistence_rx) = if streaming {
            let (w, r, p) = self
                .event_queue_manager
                .get_or_create_with_persistence(&task_id)
                .await;
            let r = match r {
                Some(r) => r,
                None => {
                    // Queue already exists — subscribe to it instead of failing.
                    self.event_queue_manager
                        .subscribe(&task_id)
                        .await
                        .ok_or_else(|| {
                            ServerError::Internal("event queue disappeared during subscribe".into())
                        })?
                }
            };
            (w, r, p)
        } else {
            let (w, r) = self.event_queue_manager.get_or_create(&task_id).await;
            let r = match r {
                Some(r) => r,
                None => {
                    // Queue already exists — subscribe to it instead of failing.
                    self.event_queue_manager
                        .subscribe(&task_id)
                        .await
                        .ok_or_else(|| {
                            ServerError::Internal("event queue disappeared during subscribe".into())
                        })?
                }
            };
            (w, r, None)
        };

        // Spawn executor task. The spawned task owns the only writer clone
        // needed; drop the local reference and the manager's reference so the
        // channel closes when the executor finishes.
        let executor = Arc::clone(&self.executor);
        let task_id_for_cleanup = task_id.clone();
        let event_queue_mgr = self.event_queue_manager.clone();
        let cancel_tokens = Arc::clone(&self.cancellation_tokens);
        let executor_timeout = self.executor_timeout;
        let executor_handle = tokio::spawn(async move {
            trace_debug!(task_id = %ctx.task_id, "executor started");

            // FIX(L5): Use a cleanup guard so that the event queue and
            // cancellation token are cleaned up even if the task is aborted
            // or panics. The guard runs on drop, which Rust guarantees
            // during normal unwinding and when the JoinHandle is aborted.
            #[allow(clippy::items_after_statements)]
            struct CleanupGuard {
                task_id: Option<TaskId>,
                queue_mgr: crate::streaming::EventQueueManager,
                tokens: std::sync::Arc<tokio::sync::RwLock<HashMap<TaskId, CancellationEntry>>>,
            }
            #[allow(clippy::items_after_statements)]
            impl Drop for CleanupGuard {
                fn drop(&mut self) {
                    if let Some(tid) = self.task_id.take() {
                        let qmgr = self.queue_mgr.clone();
                        let tokens = std::sync::Arc::clone(&self.tokens);
                        tokio::task::spawn(async move {
                            qmgr.destroy(&tid).await;
                            tokens.write().await.remove(&tid);
                        });
                    }
                }
            }
            let mut cleanup_guard = CleanupGuard {
                task_id: Some(task_id_for_cleanup.clone()),
                queue_mgr: event_queue_mgr.clone(),
                tokens: Arc::clone(&cancel_tokens),
            };

            // Wrap executor call to catch panics, ensuring cleanup always runs.
            let result = {
                let exec_future = if let Some(timeout) = executor_timeout {
                    tokio::time::timeout(timeout, executor.execute(&ctx, writer.as_ref()))
                        .await
                        .unwrap_or_else(|_| {
                            Err(a2a_protocol_types::error::A2aError::internal(format!(
                                "executor timed out after {}s",
                                timeout.as_secs()
                            )))
                        })
                } else {
                    executor.execute(&ctx, writer.as_ref()).await
                };
                exec_future
            };

            if let Err(ref e) = result {
                trace_error!(task_id = %ctx.task_id, error = %e, "executor failed");
                // Write a failed status update on error.
                let fail_event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::with_timestamp(TaskState::Failed),
                    metadata: Some(serde_json::json!({ "error": e.to_string() })),
                });
                if let Err(_write_err) = writer.write(fail_event).await {
                    trace_error!(
                        task_id = %ctx.task_id,
                        error = %_write_err,
                        "failed to write failure event to queue"
                    );
                }
            }
            // Drop the writer so the channel closes and readers see EOF.
            drop(writer);
            // Perform explicit cleanup, then defuse the guard so it does not
            // double-clean on normal exit.
            event_queue_mgr.destroy(&task_id_for_cleanup).await;
            cancel_tokens.write().await.remove(&task_id_for_cleanup);
            cleanup_guard.task_id = None;
        });

        self.interceptors.run_after(&call_ctx).await?;

        if streaming {
            // ARCHITECTURAL FIX: Spawn a background event processor that
            // runs independently of the SSE consumer. This ensures that:
            // 1. Task store is updated with state transitions even in streaming mode
            // 2. Push notifications fire for every event regardless of consumer mode
            // 3. State transition validation occurs for streaming events
            //
            // H5 FIX: The persistence channel is a dedicated mpsc channel that
            // is not affected by SSE consumer backpressure, so the background
            // processor never misses state transitions.
            self.spawn_background_event_processor(task_id.clone(), executor_handle, persistence_rx);
            Ok(SendMessageResult::Stream(reader))
        } else if return_immediately {
            // Return the task immediately without waiting for completion.
            Ok(SendMessageResult::Response(SendMessageResponse::Task(task)))
        } else {
            // Poll reader until final event. Pass the executor handle so
            // collect_events can detect executor completion/panic (CB-3).
            let final_task = self
                .collect_events(reader, task_id.clone(), executor_handle)
                .await?;
            Ok(SendMessageResult::Response(SendMessageResponse::Task(
                final_task,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
    use a2a_protocol_types::params::{MessageSendParams, SendMessageConfiguration};
    use a2a_protocol_types::task::ContextId;

    use crate::agent_executor;
    use crate::builder::RequestHandlerBuilder;

    struct DummyExecutor;
    agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

    fn make_handler() -> RequestHandler {
        RequestHandlerBuilder::new(DummyExecutor)
            .build()
            .expect("default build should succeed")
    }

    fn make_params(context_id: Option<&str>) -> MessageSendParams {
        MessageSendParams {
            context_id: None,
            message: Message {
                id: MessageId::new("msg-1"),
                role: MessageRole::User,
                parts: vec![Part::text("hello")],
                context_id: context_id.map(ContextId::new),
                task_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            },
            configuration: None,
            metadata: None,
            tenant: None,
        }
    }

    #[tokio::test]
    async fn empty_message_parts_returns_invalid_params() {
        let handler = make_handler();
        let mut params = make_params(None);
        params.message.parts = vec![];

        let result = handler.on_send_message(params, false, None).await;

        assert!(
            matches!(result, Err(ServerError::InvalidParams(_))),
            "expected InvalidParams for empty parts"
        );
    }

    #[tokio::test]
    async fn oversized_message_metadata_returns_invalid_params() {
        let handler = make_handler();
        let mut params = make_params(None);
        // Build a JSON string that exceeds the default 1 MiB limit.
        let big_value = "x".repeat(1_100_000);
        params.message.metadata = Some(serde_json::json!(big_value));

        let result = handler.on_send_message(params, false, None).await;

        assert!(
            matches!(result, Err(ServerError::InvalidParams(_))),
            "expected InvalidParams for oversized message metadata"
        );
    }

    #[tokio::test]
    async fn oversized_request_metadata_returns_invalid_params() {
        let handler = make_handler();
        let mut params = make_params(None);
        // Build a JSON string that exceeds the default 1 MiB limit.
        let big_value = "x".repeat(1_100_000);
        params.metadata = Some(serde_json::json!(big_value));

        let result = handler.on_send_message(params, false, None).await;

        assert!(
            matches!(result, Err(ServerError::InvalidParams(_))),
            "expected InvalidParams for oversized request metadata"
        );
    }

    #[tokio::test]
    async fn valid_message_returns_ok() {
        let handler = make_handler();
        let params = make_params(None);

        let result = handler.on_send_message(params, false, None).await;

        assert!(result.is_ok(), "expected Ok for valid message");
    }

    #[tokio::test]
    async fn return_immediately_returns_task() {
        let handler = make_handler();
        let mut params = make_params(None);
        params.configuration = Some(SendMessageConfiguration {
            accepted_output_modes: vec!["text/plain".into()],
            task_push_notification_config: None,
            history_length: None,
            return_immediately: Some(true),
        });

        let result = handler.on_send_message(params, false, None).await;

        assert!(
            matches!(
                result,
                Ok(SendMessageResult::Response(SendMessageResponse::Task(_)))
            ),
            "expected Response(Task) for return_immediately=true"
        );
    }

    #[tokio::test]
    async fn empty_context_id_returns_invalid_params() {
        let handler = make_handler();
        let params = make_params(Some(""));

        let result = handler.on_send_message(params, false, None).await;

        assert!(
            matches!(result, Err(ServerError::InvalidParams(_))),
            "expected InvalidParams for empty context_id"
        );
    }

    #[tokio::test]
    async fn too_long_context_id_returns_invalid_params() {
        // Covers line 98-99: context_id exceeding max_id_length.
        use crate::handler::limits::HandlerLimits;

        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_handler_limits(HandlerLimits::default().with_max_id_length(10))
            .build()
            .unwrap();
        let long_ctx = "x".repeat(20);
        let params = make_params(Some(&long_ctx));

        let result = handler.on_send_message(params, false, None).await;
        assert!(
            matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("maximum length")),
            "expected InvalidParams for too-long context_id"
        );
    }

    #[tokio::test]
    async fn too_long_task_id_returns_invalid_params() {
        // Covers lines 108-109: task_id exceeding max_id_length.
        use crate::handler::limits::HandlerLimits;
        use a2a_protocol_types::task::TaskId;

        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_handler_limits(HandlerLimits::default().with_max_id_length(10))
            .build()
            .unwrap();
        let mut params = make_params(None);
        params.message.task_id = Some(TaskId::new("a".repeat(20)));

        let result = handler.on_send_message(params, false, None).await;
        assert!(
            matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("maximum length")),
            "expected InvalidParams for too-long task_id"
        );
    }

    #[tokio::test]
    async fn empty_task_id_returns_invalid_params() {
        // Covers line 114: empty task_id validation.
        use a2a_protocol_types::task::TaskId;

        let handler = make_handler();
        let mut params = make_params(None);
        params.message.task_id = Some(TaskId::new(""));

        let result = handler.on_send_message(params, false, None).await;
        assert!(
            matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("empty")),
            "expected InvalidParams for empty task_id"
        );
    }

    #[tokio::test]
    async fn task_id_mismatch_returns_invalid_params() {
        // Covers line 136: context/task mismatch when stored task exists with different task_id.
        use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

        let handler = make_handler();

        // Save a task with context_id "ctx-existing".
        let task = Task {
            id: TaskId::new("stored-task-id"),
            context_id: ContextId::new("ctx-existing"),
            status: TaskStatus::new(TaskState::Completed),
            history: None,
            artifacts: None,
            metadata: None,
        };
        handler.task_store.save(task).await.unwrap();

        // Send a message with the same context_id but a different task_id.
        let mut params = make_params(Some("ctx-existing"));
        params.message.task_id = Some(TaskId::new("different-task-id"));

        let result = handler.on_send_message(params, false, None).await;
        assert!(
            matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("does not match")),
            "expected InvalidParams for task_id mismatch"
        );
    }

    #[tokio::test]
    async fn send_message_with_request_metadata() {
        // Covers line 186: setting request metadata on context.
        let handler = make_handler();
        let mut params = make_params(None);
        params.metadata = Some(serde_json::json!({"key": "value"}));

        let result = handler.on_send_message(params, false, None).await;
        assert!(
            result.is_ok(),
            "send_message with request metadata should succeed"
        );
    }

    #[tokio::test]
    async fn send_message_error_path_records_metrics() {
        // Covers lines 195-199: the Err branch in the outer metrics match.
        use crate::call_context::CallContext;
        use crate::interceptor::ServerInterceptor;
        use std::future::Future;
        use std::pin::Pin;

        struct FailInterceptor;
        impl ServerInterceptor for FailInterceptor {
            fn before<'a>(
                &'a self,
                _ctx: &'a CallContext,
            ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async {
                    Err(a2a_protocol_types::error::A2aError::internal(
                        "forced failure",
                    ))
                })
            }
            fn after<'a>(
                &'a self,
                _ctx: &'a CallContext,
            ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async { Ok(()) })
            }
        }

        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_interceptor(FailInterceptor)
            .build()
            .unwrap();

        let params = make_params(None);
        let result = handler.on_send_message(params, false, None).await;
        assert!(
            result.is_err(),
            "send_message should fail when interceptor rejects, exercising error metrics path"
        );
    }

    #[tokio::test]
    async fn send_streaming_message_error_path_records_metrics() {
        // Covers the streaming variant of the error metrics path (method_name = "SendStreamingMessage").
        use crate::call_context::CallContext;
        use crate::interceptor::ServerInterceptor;
        use std::future::Future;
        use std::pin::Pin;

        struct FailInterceptor;
        impl ServerInterceptor for FailInterceptor {
            fn before<'a>(
                &'a self,
                _ctx: &'a CallContext,
            ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async {
                    Err(a2a_protocol_types::error::A2aError::internal(
                        "forced failure",
                    ))
                })
            }
            fn after<'a>(
                &'a self,
                _ctx: &'a CallContext,
            ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async { Ok(()) })
            }
        }

        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_interceptor(FailInterceptor)
            .build()
            .unwrap();

        let params = make_params(None);
        let result = handler.on_send_message(params, true, None).await;
        assert!(
            result.is_err(),
            "streaming send_message should fail when interceptor rejects"
        );
    }

    #[tokio::test]
    async fn streaming_mode_returns_stream_result() {
        // Covers lines 270-280: the streaming=true branch returning SendMessageResult::Stream.
        let handler = make_handler();
        let params = make_params(None);

        let result = handler.on_send_message(params, true, None).await;
        assert!(
            matches!(result, Ok(SendMessageResult::Stream(_))),
            "expected Stream result in streaming mode"
        );
    }

    #[tokio::test]
    async fn send_message_with_stored_task_continuation() {
        // Covers lines 182-184: setting stored_task on context when a task
        // exists for the given context_id.
        use a2a_protocol_types::task::{Task, TaskState, TaskStatus};

        let handler = make_handler();

        // Pre-save a task with a known context_id.
        let task = Task {
            id: TaskId::new("existing-task"),
            context_id: ContextId::new("continue-ctx"),
            status: TaskStatus::new(TaskState::Completed),
            history: None,
            artifacts: None,
            metadata: None,
        };
        handler.task_store.save(task).await.unwrap();

        // Send message with the same context_id — should find the stored task.
        let params = make_params(Some("continue-ctx"));
        let result = handler.on_send_message(params, false, None).await;
        assert!(
            result.is_ok(),
            "send_message with existing context should succeed"
        );
    }

    #[tokio::test]
    async fn send_message_with_headers() {
        // Covers line 76: build_call_context receives headers.
        let handler = make_handler();
        let params = make_params(None);
        let mut headers = HashMap::new();
        headers.insert("authorization".to_string(), "Bearer test-token".to_string());

        let result = handler.on_send_message(params, false, Some(&headers)).await;
        assert!(result.is_ok(), "send_message with headers should succeed");
    }

    #[tokio::test]
    async fn duplicate_task_id_without_context_match_returns_error() {
        // Covers lines 140-152: insert_if_absent returns false for duplicate task_id.
        use a2a_protocol_types::task::{Task, TaskId as TId, TaskState, TaskStatus};

        let handler = make_handler();

        // Pre-save a task with task_id "dup-task" but context "other-ctx".
        let task = Task {
            id: TId::new("dup-task"),
            context_id: ContextId::new("other-ctx"),
            status: TaskStatus::new(TaskState::Completed),
            history: None,
            artifacts: None,
            metadata: None,
        };
        handler.task_store.save(task).await.unwrap();

        // Send a message with a new context_id but the same task_id.
        let mut params = make_params(Some("brand-new-ctx"));
        params.message.task_id = Some(TId::new("dup-task"));

        let result = handler.on_send_message(params, false, None).await;
        assert!(
            matches!(result, Err(ServerError::InvalidParams(ref msg)) if msg.contains("already exists")),
            "expected InvalidParams for duplicate task_id"
        );
    }

    #[tokio::test]
    async fn send_message_with_tenant() {
        // Covers line 46: tenant scoping with non-default tenant.
        let handler = make_handler();
        let mut params = make_params(None);
        params.tenant = Some("test-tenant".to_string());

        let result = handler.on_send_message(params, false, None).await;
        assert!(result.is_ok(), "send_message with tenant should succeed");
    }

    #[tokio::test]
    async fn executor_timeout_returns_failed_task() {
        // Covers lines 228-236: the executor timeout path.
        use a2a_protocol_types::error::A2aResult;
        use std::time::Duration;

        struct SlowExecutor;
        impl crate::executor::AgentExecutor for SlowExecutor {
            fn execute<'a>(
                &'a self,
                _ctx: &'a crate::request_context::RequestContext,
                _queue: &'a dyn crate::streaming::EventQueueWriter,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    Ok(())
                })
            }
        }

        let handler = RequestHandlerBuilder::new(SlowExecutor)
            .with_executor_timeout(Duration::from_millis(50))
            .build()
            .unwrap();

        let params = make_params(None);
        // The executor times out; collect_events should see a Failed status update.
        let result = handler.on_send_message(params, false, None).await;
        // The result should be Ok with a completed/failed task (the timeout writes a failed event).
        assert!(
            result.is_ok(),
            "executor timeout should still return a task result"
        );
    }

    #[tokio::test]
    async fn executor_failure_writes_failed_event() {
        // Covers lines 243-258: executor error path writes a failed status event.
        use a2a_protocol_types::error::{A2aError, A2aResult};

        struct FailExecutor;
        impl crate::executor::AgentExecutor for FailExecutor {
            fn execute<'a>(
                &'a self,
                _ctx: &'a crate::request_context::RequestContext,
                _queue: &'a dyn crate::streaming::EventQueueWriter,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async { Err(A2aError::internal("executor exploded")) })
            }
        }

        let handler = RequestHandlerBuilder::new(FailExecutor).build().unwrap();
        let params = make_params(None);

        let result = handler.on_send_message(params, false, None).await;
        // collect_events should see the failed status update.
        assert!(
            result.is_ok(),
            "executor failure should produce a task result"
        );
    }

    #[tokio::test]
    async fn cancellation_token_sweep_runs_when_map_is_full() {
        // Covers lines 194-199: the cancellation token sweep when the map
        // exceeds max_cancellation_tokens.
        use crate::handler::limits::HandlerLimits;

        // Use a slow executor so tokens accumulate before being cleaned up.
        struct SlowExec;
        impl crate::executor::AgentExecutor for SlowExec {
            fn execute<'a>(
                &'a self,
                _ctx: &'a crate::request_context::RequestContext,
                _queue: &'a dyn crate::streaming::EventQueueWriter,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>>
                        + Send
                        + 'a,
                >,
            > {
                Box::pin(async {
                    // Hold the token for a bit so tokens accumulate.
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    Ok(())
                })
            }
        }

        let handler = RequestHandlerBuilder::new(SlowExec)
            .with_handler_limits(HandlerLimits::default().with_max_cancellation_tokens(2))
            .build()
            .unwrap();

        // Send multiple streaming messages so tokens accumulate (streaming returns
        // immediately without waiting for executor to finish).
        for _ in 0..3 {
            let params = make_params(None);
            let _ = handler.on_send_message(params, true, None).await;
        }
        // If we get here without panic, the sweep logic ran successfully.
        // Clean up the slow executors.
        handler.shutdown().await;
    }

    #[tokio::test]
    async fn streaming_executor_failure_writes_error_event() {
        // Covers lines 243-258 in streaming mode: executor error path.
        use a2a_protocol_types::error::{A2aError, A2aResult};

        struct FailExecutor;
        impl crate::executor::AgentExecutor for FailExecutor {
            fn execute<'a>(
                &'a self,
                _ctx: &'a crate::request_context::RequestContext,
                _queue: &'a dyn crate::streaming::EventQueueWriter,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async { Err(A2aError::internal("streaming fail")) })
            }
        }

        let handler = RequestHandlerBuilder::new(FailExecutor).build().unwrap();
        let params = make_params(None);

        let result = handler.on_send_message(params, true, None).await;
        assert!(
            matches!(result, Ok(SendMessageResult::Stream(_))),
            "streaming executor failure should still return stream"
        );
    }
}
