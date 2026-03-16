// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

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

        let result = self
            .send_message_inner(params, streaming, method_name, headers)
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
            let meta_size = serde_json::to_string(meta).map(|s| s.len()).unwrap_or(0);
            if meta_size > max_meta {
                return Err(ServerError::InvalidParams(format!(
                    "message metadata exceeds maximum size ({meta_size} bytes, max {max_meta})"
                )));
            }
        }
        if let Some(ref meta) = params.metadata {
            let meta_size = serde_json::to_string(meta).map(|s| s.len()).unwrap_or(0);
            if meta_size > max_meta {
                return Err(ServerError::InvalidParams(format!(
                    "request metadata exceeds maximum size ({meta_size} bytes, max {max_meta})"
                )));
            }
        }

        // Generate task and context IDs.
        let task_id = TaskId::new(uuid::Uuid::new_v4().to_string());
        let context_id = params
            .message
            .context_id
            .as_ref()
            .map_or_else(|| uuid::Uuid::new_v4().to_string(), |c| c.0.clone());

        // Look up existing task for continuation.
        let stored_task = self.find_task_by_context(&context_id).await;

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

        self.task_store.save(task.clone()).await?;

        // Build request context.
        let mut ctx = RequestContext::new(params.message, task_id.clone(), context_id);
        if let Some(stored) = stored_task {
            ctx = ctx.with_stored_task(stored);
        }
        if let Some(meta) = params.metadata {
            ctx = ctx.with_metadata(meta);
        }

        // Store the cancellation token so CancelTask can signal it.
        {
            let mut tokens = self.cancellation_tokens.write().await;
            // Sweep stale tokens if the map is getting large (prevent unbounded growth
            // if executors panic and never clean up their tokens).
            if tokens.len() >= self.limits.max_cancellation_tokens {
                let now = Instant::now();
                tokens.retain(|_, entry| {
                    !entry.token.is_cancelled()
                        && now.duration_since(entry.created_at) < self.limits.max_token_age
                });
            }
            tokens.insert(
                task_id.clone(),
                CancellationEntry {
                    token: ctx.cancellation_token.clone(),
                    created_at: Instant::now(),
                },
            );
        }

        // Create event queue.
        let (writer, reader) = self.event_queue_manager.get_or_create(&task_id).await;
        let reader = reader
            .ok_or_else(|| ServerError::Internal("event queue already exists for task".into()))?;

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
            // Drop the writer and remove the manager's reference so the
            // channel closes and readers see EOF.
            drop(writer);
            event_queue_mgr.destroy(&task_id_for_cleanup).await;
            // Clean up the cancellation token.
            cancel_tokens.write().await.remove(&task_id_for_cleanup);
        });

        self.interceptors.run_after(&call_ctx).await?;

        if streaming {
            // ARCHITECTURAL FIX: Spawn a background event processor that
            // runs independently of the SSE consumer. This ensures that:
            // 1. Task store is updated with state transitions even in streaming mode
            // 2. Push notifications fire for every event regardless of consumer mode
            // 3. State transition validation occurs for streaming events
            //
            // The background processor subscribes to the same broadcast channel
            // as the SSE reader, so both consumers see every event.
            self.spawn_background_event_processor(task_id.clone(), executor_handle);
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
