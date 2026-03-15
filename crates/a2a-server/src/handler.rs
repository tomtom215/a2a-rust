// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Core request handler — protocol logic layer.
//!
//! [`RequestHandler`] wires together the executor, stores, push sender,
//! interceptors, and event queue manager to implement all A2A 0.3.0 methods.

use std::sync::Arc;

use a2a_types::agent_card::AgentCard;
use a2a_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_types::params::{
    DeletePushConfigParams, GetPushConfigParams, ListTasksParams, MessageSendParams, TaskIdParams,
    TaskQueryParams,
};
use a2a_types::push::TaskPushNotificationConfig;
use a2a_types::responses::{SendMessageResponse, TaskListResponse};
use a2a_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use crate::call_context::CallContext;
use crate::error::{ServerError, ServerResult};
use crate::executor::AgentExecutor;
use crate::interceptor::ServerInterceptorChain;
use crate::push::{PushConfigStore, PushSender};
use crate::request_context::RequestContext;
use crate::store::TaskStore;
use crate::streaming::{
    EventQueueManager, EventQueueReader, EventQueueWriter, InMemoryQueueReader,
};

/// The core protocol logic handler.
///
/// Orchestrates task lifecycle, event streaming, push notifications, and
/// interceptor chains for all A2A methods.
pub struct RequestHandler<E: AgentExecutor> {
    pub(crate) executor: Arc<E>,
    pub(crate) task_store: Box<dyn TaskStore>,
    pub(crate) push_config_store: Box<dyn PushConfigStore>,
    pub(crate) push_sender: Option<Box<dyn PushSender>>,
    pub(crate) event_queue_manager: EventQueueManager,
    pub(crate) interceptors: ServerInterceptorChain,
    pub(crate) agent_card: Option<AgentCard>,
}

impl<E: AgentExecutor> RequestHandler<E> {
    /// Handles a `message/send` or `message/stream` request.
    ///
    /// When `streaming` is `true`, the returned response wraps an SSE reader.
    /// When `false`, the method blocks until the executor finishes and returns
    /// the final task or message.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if task creation, execution spawning, or
    /// store operations fail.
    #[allow(clippy::too_many_lines)]
    pub async fn on_send_message(
        &self,
        params: MessageSendParams,
        streaming: bool,
    ) -> ServerResult<SendMessageResult> {
        let call_ctx = CallContext::new(if streaming {
            "message/stream"
        } else {
            "message/send"
        });
        self.interceptors.run_before(&call_ctx).await?;

        // Generate task and context IDs.
        let task_id = TaskId::new(uuid::Uuid::new_v4().to_string());
        let context_id = params
            .message
            .context_id
            .as_ref()
            .map_or_else(|| uuid::Uuid::new_v4().to_string(), |c| c.0.clone());

        // Look up existing task for continuation.
        let stored_task = self.find_task_by_context(&context_id).await;

        // Create initial task.
        let task = Task {
            id: task_id.clone(),
            context_id: ContextId::new(&context_id),
            status: TaskStatus::new(TaskState::Submitted),
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

        // Create event queue.
        let (writer, reader) = self.event_queue_manager.get_or_create(&task_id).await;
        let reader = reader
            .ok_or_else(|| ServerError::Internal("event queue already exists for task".into()))?;

        // Spawn executor task.
        let executor = Arc::clone(&self.executor);
        let writer_clone = Arc::clone(&writer);
        tokio::spawn(async move {
            let result = executor.execute(&ctx, writer_clone.as_ref()).await;
            if let Err(e) = result {
                // Write a failed status update on error.
                let fail_event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(&ctx.context_id),
                    state: TaskState::Failed,
                    message: None,
                    metadata: Some(serde_json::json!({ "error": e.to_string() })),
                    r#final: true,
                });
                let _ = writer_clone.write(fail_event).await;
            }
            // Close by dropping the writer (channel closes when all senders drop).
            drop(writer_clone);
        });

        self.interceptors.run_after(&call_ctx).await?;

        if streaming {
            Ok(SendMessageResult::Stream(reader))
        } else {
            // Poll reader until final event.
            let final_task = self.collect_events(reader, task_id.clone()).await?;
            Ok(SendMessageResult::Response(SendMessageResponse::Task(
                final_task,
            )))
        }
    }

    /// Handles a `tasks/get` request.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::TaskNotFound`] if the task does not exist.
    pub async fn on_get_task(&self, params: TaskQueryParams) -> ServerResult<Task> {
        let call_ctx = CallContext::new("tasks/get");
        self.interceptors.run_before(&call_ctx).await?;

        let task = self
            .task_store
            .get(&params.id)
            .await?
            .ok_or_else(|| ServerError::TaskNotFound(params.id.clone()))?;

        self.interceptors.run_after(&call_ctx).await?;
        Ok(task)
    }

    /// Handles a `tasks/list` request.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the store operation fails.
    pub async fn on_list_tasks(&self, params: ListTasksParams) -> ServerResult<TaskListResponse> {
        let call_ctx = CallContext::new("tasks/list");
        self.interceptors.run_before(&call_ctx).await?;

        let result = self.task_store.list(&params).await?;

        self.interceptors.run_after(&call_ctx).await?;
        Ok(result)
    }

    /// Handles a `tasks/cancel` request.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::TaskNotFound`] if the task does not exist, or
    /// [`ServerError::TaskNotCancelable`] if the task is in a terminal state.
    pub async fn on_cancel_task(&self, params: TaskIdParams) -> ServerResult<Task> {
        let call_ctx = CallContext::new("tasks/cancel");
        self.interceptors.run_before(&call_ctx).await?;

        let task = self
            .task_store
            .get(&params.id)
            .await?
            .ok_or_else(|| ServerError::TaskNotFound(params.id.clone()))?;

        if task.status.state.is_terminal() {
            return Err(ServerError::TaskNotCancelable(params.id));
        }

        // Build a request context for the cancel call.
        let ctx = RequestContext::new(
            a2a_types::message::Message {
                id: a2a_types::message::MessageId::new(uuid::Uuid::new_v4().to_string()),
                role: a2a_types::message::MessageRole::User,
                parts: vec![],
                task_id: Some(params.id.clone()),
                context_id: Some(task.context_id.clone()),
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            },
            params.id.clone(),
            task.context_id.0.clone(),
        );

        let (writer, _reader) = self.event_queue_manager.get_or_create(&params.id).await;
        self.executor.cancel(&ctx, writer.as_ref()).await?;

        // Update task state.
        let mut updated = task;
        updated.status = TaskStatus::new(TaskState::Canceled);
        self.task_store.save(updated.clone()).await?;

        self.interceptors.run_after(&call_ctx).await?;
        Ok(updated)
    }

    /// Handles a `tasks/resubscribe` request.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::TaskNotFound`] if the task does not exist.
    pub async fn on_resubscribe(&self, params: TaskIdParams) -> ServerResult<InMemoryQueueReader> {
        let call_ctx = CallContext::new("tasks/resubscribe");
        self.interceptors.run_before(&call_ctx).await?;

        // Verify the task exists.
        let _task = self
            .task_store
            .get(&params.id)
            .await?
            .ok_or_else(|| ServerError::TaskNotFound(params.id.clone()))?;

        let (_writer, reader) = self.event_queue_manager.get_or_create(&params.id).await;
        let reader = reader.ok_or_else(|| {
            ServerError::Internal("no event queue available for resubscribe".into())
        })?;

        self.interceptors.run_after(&call_ctx).await?;
        Ok(reader)
    }

    /// Handles a `tasks/pushNotificationConfig/set` request.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::PushNotSupported`] if no push sender is configured.
    pub async fn on_set_push_config(
        &self,
        config: TaskPushNotificationConfig,
    ) -> ServerResult<TaskPushNotificationConfig> {
        if self.push_sender.is_none() {
            return Err(ServerError::PushNotSupported);
        }
        let call_ctx = CallContext::new("tasks/pushNotificationConfig/set");
        self.interceptors.run_before(&call_ctx).await?;

        let result = self.push_config_store.set(config).await?;

        self.interceptors.run_after(&call_ctx).await?;
        Ok(result)
    }

    /// Handles a `tasks/pushNotificationConfig/get` request.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the config is not found.
    pub async fn on_get_push_config(
        &self,
        params: GetPushConfigParams,
    ) -> ServerResult<TaskPushNotificationConfig> {
        let call_ctx = CallContext::new("tasks/pushNotificationConfig/get");
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

    /// Handles a `tasks/pushNotificationConfig/list` request.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the store operation fails.
    pub async fn on_list_push_configs(
        &self,
        task_id: TaskId,
    ) -> ServerResult<Vec<TaskPushNotificationConfig>> {
        let call_ctx = CallContext::new("tasks/pushNotificationConfig/list");
        self.interceptors.run_before(&call_ctx).await?;

        let configs = self.push_config_store.list(&task_id).await?;

        self.interceptors.run_after(&call_ctx).await?;
        Ok(configs)
    }

    /// Handles a `tasks/pushNotificationConfig/delete` request.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the store operation fails.
    pub async fn on_delete_push_config(&self, params: DeletePushConfigParams) -> ServerResult<()> {
        let call_ctx = CallContext::new("tasks/pushNotificationConfig/delete");
        self.interceptors.run_before(&call_ctx).await?;

        self.push_config_store
            .delete(&params.task_id, &params.id)
            .await?;

        self.interceptors.run_after(&call_ctx).await?;
        Ok(())
    }

    /// Handles an `agent/authenticatedExtendedCard` request.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Internal`] if no agent card is configured.
    pub async fn on_get_authenticated_extended_card(&self) -> ServerResult<AgentCard> {
        let call_ctx = CallContext::new("agent/authenticatedExtendedCard");
        self.interceptors.run_before(&call_ctx).await?;

        let card = self
            .agent_card
            .clone()
            .ok_or_else(|| ServerError::Internal("no agent card configured".into()))?;

        self.interceptors.run_after(&call_ctx).await?;
        Ok(card)
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Finds a task by context ID (linear scan for in-memory store).
    async fn find_task_by_context(&self, context_id: &str) -> Option<Task> {
        let params = ListTasksParams {
            context_id: Some(ContextId::new(context_id)),
            status: None,
            page_size: Some(1),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
        };
        self.task_store
            .list(&params)
            .await
            .ok()
            .and_then(|resp| resp.tasks.into_iter().next())
    }

    /// Collects all events from a reader until the stream closes, updating the
    /// task in the store along the way. Returns the final task snapshot.
    async fn collect_events(
        &self,
        mut reader: InMemoryQueueReader,
        task_id: TaskId,
    ) -> ServerResult<Task> {
        let mut last_task = self
            .task_store
            .get(&task_id)
            .await?
            .ok_or_else(|| ServerError::TaskNotFound(task_id.clone()))?;

        while let Some(event) = reader.read().await {
            match event {
                Ok(StreamResponse::StatusUpdate(update)) => {
                    last_task.status = TaskStatus {
                        state: update.state,
                        message: update.message,
                        timestamp: None,
                    };
                    self.task_store.save(last_task.clone()).await?;
                }
                Ok(StreamResponse::ArtifactUpdate(update)) => {
                    let artifacts = last_task.artifacts.get_or_insert_with(Vec::new);
                    artifacts.push(update.artifact);
                    self.task_store.save(last_task.clone()).await?;
                }
                Ok(StreamResponse::Task(task)) => {
                    last_task = task;
                    self.task_store.save(last_task.clone()).await?;
                }
                Ok(StreamResponse::Message(_)) => {
                    // Messages are part of history; for now just continue.
                }
                Err(e) => {
                    last_task.status = TaskStatus::new(TaskState::Failed);
                    self.task_store.save(last_task.clone()).await?;
                    return Err(ServerError::Protocol(e));
                }
            }
        }

        Ok(last_task)
    }
}

impl<E: AgentExecutor> std::fmt::Debug for RequestHandler<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestHandler")
            .field("executor", &"...")
            .field("task_store", &"...")
            .field("push_config_store", &"...")
            .field("push_sender", &self.push_sender.is_some())
            .field("event_queue_manager", &self.event_queue_manager)
            .field("interceptors", &self.interceptors)
            .field("agent_card", &self.agent_card.is_some())
            .finish()
    }
}

/// Result of [`RequestHandler::on_send_message`].
///
/// Either a synchronous response or a streaming reader.
#[allow(clippy::large_enum_variant)]
pub enum SendMessageResult {
    /// A synchronous JSON-RPC response.
    Response(SendMessageResponse),
    /// A streaming SSE reader.
    Stream(InMemoryQueueReader),
}
