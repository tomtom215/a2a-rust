// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task lifecycle methods: get, list, cancel, resubscribe, extended agent card.

use std::collections::HashMap;
use std::time::Instant;

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::params::{
    CancelTaskParams, ListTasksParams, TaskIdParams, TaskQueryParams,
};
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

use crate::error::{ServerError, ServerResult};
use crate::request_context::RequestContext;
use crate::streaming::InMemoryQueueReader;

use super::helpers::build_call_context;
use super::RequestHandler;

impl RequestHandler {
    /// Handles `GetTask`. Returns [`ServerError::TaskNotFound`] if missing.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::TaskNotFound`] if the task does not exist.
    pub async fn on_get_task(
        &self,
        params: TaskQueryParams,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<Task> {
        let start = Instant::now();
        trace_info!(method = "GetTask", task_id = %params.id, "handling get task");
        self.metrics.on_request("GetTask");

        let result: ServerResult<_> = async {
            let call_ctx = build_call_context("GetTask", headers);
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

        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response("GetTask");
                self.metrics.on_latency("GetTask", elapsed);
            }
            Err(e) => {
                self.metrics.on_error("GetTask", &e.to_string());
                self.metrics.on_latency("GetTask", elapsed);
            }
        }
        result
    }

    /// Handles `ListTasks`.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the store query fails.
    pub async fn on_list_tasks(
        &self,
        params: ListTasksParams,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<TaskListResponse> {
        let start = Instant::now();
        trace_info!(method = "ListTasks", "handling list tasks");
        self.metrics.on_request("ListTasks");

        let result: ServerResult<_> = async {
            let call_ctx = build_call_context("ListTasks", headers);
            self.interceptors.run_before(&call_ctx).await?;
            let result = self.task_store.list(&params).await?;
            self.interceptors.run_after(&call_ctx).await?;
            Ok(result)
        }
        .await;

        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response("ListTasks");
                self.metrics.on_latency("ListTasks", elapsed);
            }
            Err(e) => {
                self.metrics.on_error("ListTasks", &e.to_string());
                self.metrics.on_latency("ListTasks", elapsed);
            }
        }
        result
    }

    /// Handles `CancelTask`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::TaskNotFound`] or [`ServerError::TaskNotCancelable`].
    pub async fn on_cancel_task(
        &self,
        params: CancelTaskParams,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<Task> {
        let start = Instant::now();
        trace_info!(method = "CancelTask", task_id = %params.id, "handling cancel task");
        self.metrics.on_request("CancelTask");

        let result: ServerResult<_> = async {
            let call_ctx = build_call_context("CancelTask", headers);
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

        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response("CancelTask");
                self.metrics.on_latency("CancelTask", elapsed);
            }
            Err(e) => {
                self.metrics.on_error("CancelTask", &e.to_string());
                self.metrics.on_latency("CancelTask", elapsed);
            }
        }
        result
    }

    /// Handles `SubscribeToTask`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::TaskNotFound`] if the task does not exist.
    pub async fn on_resubscribe(
        &self,
        params: TaskIdParams,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<InMemoryQueueReader> {
        let start = Instant::now();
        trace_info!(method = "SubscribeToTask", task_id = %params.id, "handling resubscribe");
        self.metrics.on_request("SubscribeToTask");

        let result: ServerResult<_> = async {
            let call_ctx = build_call_context("SubscribeToTask", headers);
            self.interceptors.run_before(&call_ctx).await?;

            let task_id = TaskId::new(&params.id);

            // Verify the task exists.
            let _task = self
                .task_store
                .get(&task_id)
                .await?
                .ok_or_else(|| ServerError::TaskNotFound(task_id.clone()))?;

            let reader = self
                .event_queue_manager
                .subscribe(&task_id)
                .await
                .ok_or_else(|| ServerError::Internal("no active event queue for task".into()))?;

            self.interceptors.run_after(&call_ctx).await?;
            Ok(reader)
        }
        .await;

        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response("SubscribeToTask");
                self.metrics.on_latency("SubscribeToTask", elapsed);
            }
            Err(e) => {
                self.metrics.on_error("SubscribeToTask", &e.to_string());
                self.metrics.on_latency("SubscribeToTask", elapsed);
            }
        }
        result
    }

    /// Handles `GetExtendedAgentCard`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Internal`] if no agent card is configured.
    pub async fn on_get_extended_agent_card(
        &self,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<AgentCard> {
        let start = Instant::now();
        self.metrics.on_request("GetExtendedAgentCard");

        let result: ServerResult<_> = async {
            let call_ctx = build_call_context("GetExtendedAgentCard", headers);
            self.interceptors.run_before(&call_ctx).await?;

            let card = self
                .agent_card
                .clone()
                .ok_or_else(|| ServerError::Internal("no agent card configured".into()))?;

            self.interceptors.run_after(&call_ctx).await?;
            Ok(card)
        }
        .await;

        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response("GetExtendedAgentCard");
                self.metrics.on_latency("GetExtendedAgentCard", elapsed);
            }
            Err(e) => {
                self.metrics
                    .on_error("GetExtendedAgentCard", &e.to_string());
                self.metrics.on_latency("GetExtendedAgentCard", elapsed);
            }
        }
        result
    }
}
