// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task management client methods.
//!
//! Provides `get_task`, `list_tasks`, `cancel_task`, and `resubscribe` on
//! [`A2aClient`].

use a2a_types::{ListTasksParams, Task, TaskId, TaskIdParams, TaskListResponse, TaskQueryParams};

use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};
use crate::interceptor::{ClientRequest, ClientResponse};
use crate::streaming::EventStream;

impl A2aClient {
    /// Retrieves a task by ID.
    ///
    /// Calls the `tasks/get` JSON-RPC method.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Protocol`] with [`a2a_types::ErrorCode::TaskNotFound`]
    /// if no task with the given ID exists.
    pub async fn get_task(&self, params: TaskQueryParams) -> ClientResult<Task> {
        const METHOD: &str = "tasks/get";

        let params_value = serde_json::to_value(&params).map_err(ClientError::Serialization)?;

        let mut req = ClientRequest::new(METHOD, params_value);
        self.interceptors.run_before(&mut req).await?;

        let result = self
            .transport
            .send_request(METHOD, req.params, &req.extra_headers)
            .await?;

        let resp = ClientResponse {
            method: METHOD.to_owned(),
            result: result.clone(),
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<Task>(result).map_err(ClientError::Serialization)
    }

    /// Lists tasks visible to the caller.
    ///
    /// Calls the `tasks/list` JSON-RPC method. Results are paginated; use
    /// `params.page_token` to fetch subsequent pages.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or protocol errors.
    pub async fn list_tasks(&self, params: ListTasksParams) -> ClientResult<TaskListResponse> {
        const METHOD: &str = "tasks/list";

        let params_value = serde_json::to_value(&params).map_err(ClientError::Serialization)?;

        let mut req = ClientRequest::new(METHOD, params_value);
        self.interceptors.run_before(&mut req).await?;

        let result = self
            .transport
            .send_request(METHOD, req.params, &req.extra_headers)
            .await?;

        let resp = ClientResponse {
            method: METHOD.to_owned(),
            result: result.clone(),
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<TaskListResponse>(result).map_err(ClientError::Serialization)
    }

    /// Requests cancellation of a running task.
    ///
    /// Calls the `tasks/cancel` JSON-RPC method. Returns the task in its
    /// post-cancellation state.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Protocol`] with
    /// [`a2a_types::ErrorCode::TaskNotCancelable`] if the task cannot be
    /// canceled in its current state.
    pub async fn cancel_task(&self, id: TaskId) -> ClientResult<Task> {
        const METHOD: &str = "tasks/cancel";

        let params = TaskIdParams { id };
        let params_value = serde_json::to_value(&params).map_err(ClientError::Serialization)?;

        let mut req = ClientRequest::new(METHOD, params_value);
        self.interceptors.run_before(&mut req).await?;

        let result = self
            .transport
            .send_request(METHOD, req.params, &req.extra_headers)
            .await?;

        let resp = ClientResponse {
            method: METHOD.to_owned(),
            result: result.clone(),
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<Task>(result).map_err(ClientError::Serialization)
    }

    /// Resubscribes to the SSE stream for an in-progress task.
    ///
    /// Calls the `tasks/resubscribe` method. Useful after an unexpected
    /// disconnection from a `message/stream` call.
    ///
    /// Events already delivered before the reconnect are **not** replayed.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Protocol`] with
    /// [`a2a_types::ErrorCode::TaskNotFound`] if the task is not in a
    /// streaming-eligible state.
    pub async fn resubscribe(&self, id: TaskId) -> ClientResult<EventStream> {
        const METHOD: &str = "tasks/resubscribe";

        let params = TaskIdParams { id };
        let params_value = serde_json::to_value(&params).map_err(ClientError::Serialization)?;

        let mut req = ClientRequest::new(METHOD, params_value);
        self.interceptors.run_before(&mut req).await?;

        self.transport
            .send_streaming_request(METHOD, req.params, &req.extra_headers)
            .await
    }
}
