// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Core request handler — protocol logic layer.
//!
//! [`RequestHandler`] wires together the executor, stores, push sender,
//! interceptors, and event queue manager to implement all A2A v1.0 methods.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::params::{
    CancelTaskParams, DeletePushConfigParams, GetPushConfigParams, ListTasksParams,
    MessageSendParams, TaskIdParams, TaskQueryParams,
};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::{SendMessageResponse, TaskListResponse};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use crate::call_context::CallContext;
use crate::error::{ServerError, ServerResult};
use crate::executor::AgentExecutor;
use crate::interceptor::ServerInterceptorChain;
use crate::metrics::Metrics;
use crate::push::{PushConfigStore, PushSender};
use crate::request_context::RequestContext;
use crate::store::TaskStore;
use crate::streaming::{
    EventQueueManager, EventQueueReader, EventQueueWriter, InMemoryQueueReader,
};

/// Maximum allowed length for task/context IDs (prevents memory exhaustion).
const MAX_ID_LENGTH: usize = 1024;

/// Maximum allowed serialized size for metadata fields (1 MiB).
const MAX_METADATA_SIZE: usize = 1_048_576;

/// Maximum number of entries in the cancellation token map before a cleanup
/// sweep is triggered to remove cancelled/dropped tokens.
const MAX_CANCELLATION_TOKENS: usize = 10_000;

/// Maximum age for cancellation tokens. Tokens older than this are evicted
/// during cleanup sweeps, even if they haven't been explicitly cancelled.
const MAX_TOKEN_AGE: Duration = Duration::from_secs(3600);

/// Validates an ID string: rejects empty/whitespace-only and excessively long values.
fn validate_id(raw: &str, name: &str) -> ServerResult<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ServerError::InvalidParams(format!(
            "{name} must not be empty or whitespace-only"
        )));
    }
    if trimmed.len() > MAX_ID_LENGTH {
        return Err(ServerError::InvalidParams(format!(
            "{name} exceeds maximum length (got {}, max {MAX_ID_LENGTH})",
            trimmed.len()
        )));
    }
    Ok(())
}

/// The core protocol logic handler.
///
/// Orchestrates task lifecycle, event streaming, push notifications, and
/// interceptor chains for all A2A methods.
///
/// `RequestHandler` is **not** generic — it stores the executor as
/// `Arc<dyn AgentExecutor>`, enabling dynamic dispatch and simplifying
/// the downstream API (dispatchers, builder, etc.).
pub struct RequestHandler {
    pub(crate) executor: Arc<dyn AgentExecutor>,
    pub(crate) task_store: Box<dyn TaskStore>,
    pub(crate) push_config_store: Box<dyn PushConfigStore>,
    pub(crate) push_sender: Option<Box<dyn PushSender>>,
    pub(crate) event_queue_manager: EventQueueManager,
    pub(crate) interceptors: ServerInterceptorChain,
    pub(crate) agent_card: Option<AgentCard>,
    pub(crate) executor_timeout: Option<Duration>,
    pub(crate) metrics: Arc<dyn Metrics>,
    /// Cancellation tokens for in-flight tasks (keyed by [`TaskId`]).
    pub(crate) cancellation_tokens: Arc<tokio::sync::RwLock<HashMap<TaskId, CancellationEntry>>>,
}

/// Entry in the cancellation token map, tracking creation time for eviction.
#[derive(Debug, Clone)]
pub(crate) struct CancellationEntry {
    /// The cancellation token.
    pub(crate) token: tokio_util::sync::CancellationToken,
    /// When this entry was created (for time-based eviction).
    pub(crate) created_at: Instant,
}

impl RequestHandler {
    /// Handles `SendMessage` / `SendStreamingMessage`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError`] if task creation or execution fails.
    pub async fn on_send_message(
        &self,
        params: MessageSendParams,
        streaming: bool,
    ) -> ServerResult<SendMessageResult> {
        let method_name = if streaming {
            "SendStreamingMessage"
        } else {
            "SendMessage"
        };
        trace_info!(method = method_name, streaming, "handling send message");
        self.metrics.on_request(method_name);

        let result = self
            .send_message_inner(params, streaming, method_name)
            .await;
        match &result {
            Ok(_) => self.metrics.on_response(method_name),
            Err(e) => self.metrics.on_error(method_name, &e.to_string()),
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
    ) -> ServerResult<SendMessageResult> {
        let call_ctx = CallContext::new(method_name);
        self.interceptors.run_before(&call_ctx).await?;

        // Validate incoming IDs: reject empty/whitespace-only and excessively long values (AP-1).
        if let Some(ref ctx_id) = params.message.context_id {
            validate_id(&ctx_id.0, "context_id")?;
        }
        if let Some(ref task_id) = params.message.task_id {
            validate_id(&task_id.0, "task_id")?;
        }

        // SC-4: Reject messages with no parts.
        if params.message.parts.is_empty() {
            return Err(ServerError::InvalidParams(
                "message must contain at least one part".into(),
            ));
        }

        // PR-8: Reject oversized metadata to prevent memory exhaustion.
        if let Some(ref meta) = params.message.metadata {
            let meta_size = serde_json::to_string(meta).map(|s| s.len()).unwrap_or(0);
            if meta_size > MAX_METADATA_SIZE {
                return Err(ServerError::InvalidParams(format!(
                    "message metadata exceeds maximum size ({meta_size} bytes, max {MAX_METADATA_SIZE})"
                )));
            }
        }
        if let Some(ref meta) = params.metadata {
            let meta_size = serde_json::to_string(meta).map(|s| s.len()).unwrap_or(0);
            if meta_size > MAX_METADATA_SIZE {
                return Err(ServerError::InvalidParams(format!(
                    "request metadata exceeds maximum size ({meta_size} bytes, max {MAX_METADATA_SIZE})"
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
            if tokens.len() >= MAX_CANCELLATION_TOKENS {
                let now = Instant::now();
                tokens.retain(|_, entry| {
                    !entry.token.is_cancelled()
                        && now.duration_since(entry.created_at) < MAX_TOKEN_AGE
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

    /// Handles `GetTask`. Returns [`ServerError::TaskNotFound`] if missing.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::TaskNotFound`] if the task does not exist.
    pub async fn on_get_task(&self, params: TaskQueryParams) -> ServerResult<Task> {
        trace_info!(method = "GetTask", task_id = %params.id, "handling get task");
        self.metrics.on_request("GetTask");

        let result: ServerResult<_> = async {
            let call_ctx = CallContext::new("GetTask");
            self.interceptors.run_before(&call_ctx).await?;

            let task_id = TaskId::new(&params.id);
            let task = self
                .task_store
                .get(&task_id)
                .await?
                .ok_or_else(|| ServerError::TaskNotFound(task_id))?;

            self.interceptors.run_after(&call_ctx).await?;
            Ok(task)
        }
        .await;

        match &result {
            Ok(_) => self.metrics.on_response("GetTask"),
            Err(e) => self.metrics.on_error("GetTask", &e.to_string()),
        }
        result
    }

    /// Handles `ListTasks`.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the store query fails.
    pub async fn on_list_tasks(&self, params: ListTasksParams) -> ServerResult<TaskListResponse> {
        trace_info!(method = "ListTasks", "handling list tasks");
        self.metrics.on_request("ListTasks");

        let result: ServerResult<_> = async {
            let call_ctx = CallContext::new("ListTasks");
            self.interceptors.run_before(&call_ctx).await?;
            let result = self.task_store.list(&params).await?;
            self.interceptors.run_after(&call_ctx).await?;
            Ok(result)
        }
        .await;

        match &result {
            Ok(_) => self.metrics.on_response("ListTasks"),
            Err(e) => self.metrics.on_error("ListTasks", &e.to_string()),
        }
        result
    }

    /// Handles `CancelTask`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::TaskNotFound`] or [`ServerError::TaskNotCancelable`].
    pub async fn on_cancel_task(&self, params: CancelTaskParams) -> ServerResult<Task> {
        trace_info!(method = "CancelTask", task_id = %params.id, "handling cancel task");
        self.metrics.on_request("CancelTask");

        let result: ServerResult<_> = async {
            let call_ctx = CallContext::new("CancelTask");
            self.interceptors.run_before(&call_ctx).await?;

            let task_id = TaskId::new(&params.id);
            let task = self
                .task_store
                .get(&task_id)
                .await?
                .ok_or_else(|| ServerError::TaskNotFound(task_id.clone()))?;

            if task.status.state.is_terminal() {
                return Err(ServerError::TaskNotCancelable(task_id));
            }

            // Signal the cancellation token so the executor can observe the cancellation.
            {
                let tokens = self.cancellation_tokens.read().await;
                if let Some(entry) = tokens.get(&task_id) {
                    entry.token.cancel();
                }
            }

            // Build a request context for the cancel call.
            let ctx = RequestContext::new(
                a2a_protocol_types::message::Message {
                    id: a2a_protocol_types::message::MessageId::new(
                        uuid::Uuid::new_v4().to_string(),
                    ),
                    role: a2a_protocol_types::message::MessageRole::User,
                    parts: vec![],
                    task_id: Some(task_id.clone()),
                    context_id: Some(task.context_id.clone()),
                    reference_task_ids: None,
                    extensions: None,
                    metadata: None,
                },
                task_id.clone(),
                task.context_id.0.clone(),
            );

            let (writer, _reader) = self.event_queue_manager.get_or_create(&task_id).await;
            self.executor.cancel(&ctx, writer.as_ref()).await?;

            // Update task state.
            let mut updated = task;
            updated.status = TaskStatus::with_timestamp(TaskState::Canceled);
            self.task_store.save(updated.clone()).await?;

            self.interceptors.run_after(&call_ctx).await?;
            Ok(updated)
        }
        .await;

        match &result {
            Ok(_) => self.metrics.on_response("CancelTask"),
            Err(e) => self.metrics.on_error("CancelTask", &e.to_string()),
        }
        result
    }

    /// Handles `SubscribeToTask`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::TaskNotFound`] if the task does not exist.
    pub async fn on_resubscribe(&self, params: TaskIdParams) -> ServerResult<InMemoryQueueReader> {
        trace_info!(method = "SubscribeToTask", task_id = %params.id, "handling resubscribe");
        self.metrics.on_request("SubscribeToTask");

        let result: ServerResult<_> = async {
            let call_ctx = CallContext::new("SubscribeToTask");
            self.interceptors.run_before(&call_ctx).await?;

            let task_id = TaskId::new(&params.id);

            // Verify the task exists.
            let _task = self
                .task_store
                .get(&task_id)
                .await?
                .ok_or_else(|| ServerError::TaskNotFound(task_id.clone()))?;

            let (_writer, reader) = self.event_queue_manager.get_or_create(&task_id).await;
            let reader = reader.ok_or_else(|| {
                ServerError::Internal("no event queue available for resubscribe".into())
            })?;

            self.interceptors.run_after(&call_ctx).await?;
            Ok(reader)
        }
        .await;

        match &result {
            Ok(_) => self.metrics.on_response("SubscribeToTask"),
            Err(e) => self.metrics.on_error("SubscribeToTask", &e.to_string()),
        }
        result
    }

    /// Handles `CreateTaskPushNotificationConfig`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::PushNotSupported`] if no push sender is configured.
    pub async fn on_set_push_config(
        &self,
        config: TaskPushNotificationConfig,
    ) -> ServerResult<TaskPushNotificationConfig> {
        self.metrics.on_request("CreateTaskPushNotificationConfig");

        let result: ServerResult<_> = async {
            if self.push_sender.is_none() {
                return Err(ServerError::PushNotSupported);
            }
            let call_ctx = CallContext::new("CreateTaskPushNotificationConfig");
            self.interceptors.run_before(&call_ctx).await?;
            let result = self.push_config_store.set(config).await?;
            self.interceptors.run_after(&call_ctx).await?;
            Ok(result)
        }
        .await;

        match &result {
            Ok(_) => self.metrics.on_response("CreateTaskPushNotificationConfig"),
            Err(e) => {
                self.metrics
                    .on_error("CreateTaskPushNotificationConfig", &e.to_string());
            }
        }
        result
    }

    /// Handles `GetTaskPushNotificationConfig`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::InvalidParams`] if the config is not found.
    pub async fn on_get_push_config(
        &self,
        params: GetPushConfigParams,
    ) -> ServerResult<TaskPushNotificationConfig> {
        self.metrics.on_request("GetTaskPushNotificationConfig");

        let result: ServerResult<_> = async {
            let call_ctx = CallContext::new("GetTaskPushNotificationConfig");
            self.interceptors.run_before(&call_ctx).await?;

            let config = self
                .push_config_store
                .get(&params.task_id, &params.id)
                .await?
                .ok_or_else(|| {
                    ServerError::InvalidParams(format!(
                        "push config not found: task={}, id={}",
                        params.task_id, params.id
                    ))
                })?;

            self.interceptors.run_after(&call_ctx).await?;
            Ok(config)
        }
        .await;

        match &result {
            Ok(_) => self.metrics.on_response("GetTaskPushNotificationConfig"),
            Err(e) => {
                self.metrics
                    .on_error("GetTaskPushNotificationConfig", &e.to_string());
            }
        }
        result
    }

    /// Handles `ListTaskPushNotificationConfigs`.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the store query fails.
    pub async fn on_list_push_configs(
        &self,
        task_id: &str,
    ) -> ServerResult<Vec<TaskPushNotificationConfig>> {
        self.metrics.on_request("ListTaskPushNotificationConfigs");

        let result: ServerResult<_> = async {
            let call_ctx = CallContext::new("ListTaskPushNotificationConfigs");
            self.interceptors.run_before(&call_ctx).await?;
            let configs = self.push_config_store.list(task_id).await?;
            self.interceptors.run_after(&call_ctx).await?;
            Ok(configs)
        }
        .await;

        match &result {
            Ok(_) => self.metrics.on_response("ListTaskPushNotificationConfigs"),
            Err(e) => {
                self.metrics
                    .on_error("ListTaskPushNotificationConfigs", &e.to_string());
            }
        }
        result
    }

    /// Handles `DeleteTaskPushNotificationConfig`.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the delete operation fails.
    pub async fn on_delete_push_config(&self, params: DeletePushConfigParams) -> ServerResult<()> {
        self.metrics.on_request("DeleteTaskPushNotificationConfig");

        let result: ServerResult<_> = async {
            let call_ctx = CallContext::new("DeleteTaskPushNotificationConfig");
            self.interceptors.run_before(&call_ctx).await?;
            self.push_config_store
                .delete(&params.task_id, &params.id)
                .await?;
            self.interceptors.run_after(&call_ctx).await?;
            Ok(())
        }
        .await;

        match &result {
            Ok(()) => self.metrics.on_response("DeleteTaskPushNotificationConfig"),
            Err(e) => {
                self.metrics
                    .on_error("DeleteTaskPushNotificationConfig", &e.to_string());
            }
        }
        result
    }

    /// Handles `GetExtendedAgentCard`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Internal`] if no agent card is configured.
    pub async fn on_get_extended_agent_card(&self) -> ServerResult<AgentCard> {
        self.metrics.on_request("GetExtendedAgentCard");

        let result: ServerResult<_> = async {
            let call_ctx = CallContext::new("GetExtendedAgentCard");
            self.interceptors.run_before(&call_ctx).await?;

            let card = self
                .agent_card
                .clone()
                .ok_or_else(|| ServerError::Internal("no agent card configured".into()))?;

            self.interceptors.run_after(&call_ctx).await?;
            Ok(card)
        }
        .await;

        match &result {
            Ok(_) => self.metrics.on_response("GetExtendedAgentCard"),
            Err(e) => self
                .metrics
                .on_error("GetExtendedAgentCard", &e.to_string()),
        }
        result
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Finds a task by context ID (linear scan for in-memory store).
    async fn find_task_by_context(&self, context_id: &str) -> Option<Task> {
        if context_id.len() > MAX_ID_LENGTH {
            return None;
        }
        let params = ListTasksParams {
            tenant: None,
            context_id: Some(context_id.to_owned()),
            status: None,
            page_size: Some(1),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        };
        self.task_store
            .list(&params)
            .await
            .ok()
            .and_then(|resp| resp.tasks.into_iter().next())
    }

    /// Collects events until stream closes, updating the task store and
    /// delivering push notifications. Returns the final task.
    ///
    /// Takes the executor's `JoinHandle` so that if the executor panics or
    /// terminates without closing the queue properly, we detect it and avoid
    /// blocking forever (CB-3).
    async fn collect_events(
        &self,
        mut reader: InMemoryQueueReader,
        task_id: TaskId,
        executor_handle: tokio::task::JoinHandle<()>,
    ) -> ServerResult<Task> {
        let mut last_task = self
            .task_store
            .get(&task_id)
            .await?
            .ok_or_else(|| ServerError::TaskNotFound(task_id.clone()))?;

        // Pin the executor handle so we can poll it alongside the reader.
        // When the executor finishes (or panics), we'll drain remaining events
        // and then return, rather than blocking forever.
        let mut executor_done = false;
        let mut handle_fuse = executor_handle;

        loop {
            if executor_done {
                // Executor finished — drain any remaining buffered events.
                match reader.read().await {
                    Some(event) => {
                        self.process_event(event, &task_id, &mut last_task).await?;
                    }
                    None => break,
                }
            } else {
                tokio::select! {
                    biased;
                    event = reader.read() => {
                        match event {
                            Some(event) => {
                                self.process_event(event, &task_id, &mut last_task).await?;
                            }
                            None => break,
                        }
                    }
                    result = &mut handle_fuse => {
                        executor_done = true;
                        if result.is_err() {
                            // Executor panicked (CB-2). Mark task as failed
                            // and drain remaining events.
                            trace_error!(
                                task_id = %task_id,
                                "executor task panicked"
                            );
                            if !last_task.status.state.is_terminal() {
                                last_task.status = TaskStatus::with_timestamp(TaskState::Failed);
                                self.task_store.save(last_task.clone()).await?;
                            }
                        }
                        // Continue to drain remaining events from the queue.
                    }
                }
            }
        }

        Ok(last_task)
    }

    /// Processes a single event from the queue reader, updating the task and
    /// delivering push notifications.
    async fn process_event(
        &self,
        event: a2a_protocol_types::error::A2aResult<StreamResponse>,
        task_id: &TaskId,
        last_task: &mut Task,
    ) -> ServerResult<()> {
        match event {
            Ok(ref stream_resp @ StreamResponse::StatusUpdate(ref update)) => {
                let current = last_task.status.state;
                let next = update.status.state;
                if !current.can_transition_to(next) {
                    trace_warn!(
                        task_id = %task_id,
                        from = %current,
                        to = %next,
                        "invalid state transition rejected"
                    );
                    return Err(ServerError::InvalidStateTransition {
                        task_id: task_id.clone(),
                        from: current,
                        to: next,
                    });
                }
                last_task.status = TaskStatus {
                    state: next,
                    message: update.status.message.clone(),
                    timestamp: update.status.timestamp.clone(),
                };
                self.task_store.save(last_task.clone()).await?;
                self.deliver_push(task_id, stream_resp).await;
            }
            Ok(ref stream_resp @ StreamResponse::ArtifactUpdate(ref update)) => {
                let artifacts = last_task.artifacts.get_or_insert_with(Vec::new);
                artifacts.push(update.artifact.clone());
                self.task_store.save(last_task.clone()).await?;
                self.deliver_push(task_id, stream_resp).await;
            }
            Ok(StreamResponse::Task(task)) => {
                *last_task = task;
                self.task_store.save(last_task.clone()).await?;
            }
            Ok(StreamResponse::Message(_) | _) => {
                // Messages and future stream response variants — continue.
            }
            Err(e) => {
                last_task.status = TaskStatus::with_timestamp(TaskState::Failed);
                self.task_store.save(last_task.clone()).await?;
                return Err(ServerError::Protocol(e));
            }
        }
        Ok(())
    }

    /// Delivers push notifications for a streaming event if configs exist.
    ///
    /// Push deliveries are sequential per-config, but each delivery is bounded
    /// by a 5-second timeout to prevent one slow webhook from blocking all
    /// subsequent deliveries indefinitely.
    async fn deliver_push(&self, task_id: &TaskId, event: &StreamResponse) {
        let Some(ref sender) = self.push_sender else {
            return;
        };
        let Ok(configs) = self.push_config_store.list(task_id.as_ref()).await else {
            return;
        };
        for config in &configs {
            // Bound each push delivery to prevent one slow webhook from blocking all others.
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                sender.send(&config.url, event, config),
            )
            .await;
            match result {
                Ok(Err(_err)) => {
                    trace_warn!(
                        task_id = %task_id,
                        url = %config.url,
                        error = %_err,
                        "push notification delivery failed"
                    );
                }
                Err(_) => {
                    trace_warn!(
                        task_id = %task_id,
                        url = %config.url,
                        "push notification delivery timed out"
                    );
                }
                Ok(Ok(())) => {}
            }
        }
    }
}

impl RequestHandler {
    /// Initiates graceful shutdown of the handler.
    ///
    /// This method:
    /// 1. Cancels all in-flight tasks by signalling their cancellation tokens.
    /// 2. Destroys all event queues, causing readers to see EOF.
    ///
    /// After calling `shutdown()`, new requests will still be accepted but
    /// in-flight tasks will observe cancellation. The caller should stop
    /// accepting new connections after calling this method.
    pub async fn shutdown(&self) {
        // Cancel all in-flight tasks.
        {
            let tokens = self.cancellation_tokens.read().await;
            for entry in tokens.values() {
                entry.token.cancel();
            }
        }

        // Destroy all event queues so readers see EOF.
        self.event_queue_manager.destroy_all().await;

        // Clear cancellation tokens.
        {
            let mut tokens = self.cancellation_tokens.write().await;
            tokens.clear();
        }

        // Give executor a chance to clean up resources.
        self.executor.on_shutdown().await;
    }

    /// Initiates graceful shutdown with a timeout.
    ///
    /// Cancels all in-flight tasks and waits up to `timeout` for event queues
    /// to drain before force-destroying them. This gives executors a chance
    /// to finish writing final events before the queues are torn down.
    pub async fn shutdown_with_timeout(&self, timeout: Duration) {
        // Cancel all in-flight tasks.
        {
            let tokens = self.cancellation_tokens.read().await;
            for entry in tokens.values() {
                entry.token.cancel();
            }
        }

        // Wait for event queues to drain (executors to finish), with timeout.
        let drain_start = Instant::now();
        loop {
            let active = self.event_queue_manager.active_count().await;
            if active == 0 {
                break;
            }
            if drain_start.elapsed() >= timeout {
                trace_warn!(
                    active_queues = active,
                    "shutdown timeout reached, force-destroying remaining queues"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Destroy all remaining event queues.
        self.event_queue_manager.destroy_all().await;

        // Clear cancellation tokens.
        {
            let mut tokens = self.cancellation_tokens.write().await;
            tokens.clear();
        }

        // Give executor a chance to clean up resources.
        self.executor.on_shutdown().await;
    }
}

impl std::fmt::Debug for RequestHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestHandler")
            .field("push_sender", &self.push_sender.is_some())
            .field("event_queue_manager", &self.event_queue_manager)
            .field("interceptors", &self.interceptors)
            .field("agent_card", &self.agent_card.is_some())
            .field("metrics", &"<dyn Metrics>")
            .finish_non_exhaustive()
    }
}

/// Result of [`RequestHandler::on_send_message`].
#[allow(clippy::large_enum_variant)]
pub enum SendMessageResult {
    /// A synchronous JSON-RPC response.
    Response(SendMessageResponse),
    /// A streaming SSE reader.
    Stream(InMemoryQueueReader),
}
