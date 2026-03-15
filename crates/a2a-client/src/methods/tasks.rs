// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task management client methods.
//!
//! Provides `get_task`, `list_tasks`, `cancel_task`, and `subscribe_to_task`
//! on [`A2aClient`].

use a2a_types::{
    CancelTaskParams, ListTasksParams, Task, TaskIdParams, TaskListResponse, TaskQueryParams,
};

use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};
use crate::interceptor::{ClientRequest, ClientResponse};
use crate::streaming::EventStream;

impl A2aClient {
    /// Retrieves a task by ID.
    ///
    /// Calls the `GetTask` JSON-RPC method.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Protocol`] with [`a2a_types::ErrorCode::TaskNotFound`]
    /// if no task with the given ID exists.
    pub async fn get_task(&self, params: TaskQueryParams) -> ClientResult<Task> {
        const METHOD: &str = "GetTask";

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
    /// Calls the `ListTasks` JSON-RPC method. Results are paginated; use
    /// `params.page_token` to fetch subsequent pages.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or protocol errors.
    pub async fn list_tasks(&self, params: ListTasksParams) -> ClientResult<TaskListResponse> {
        const METHOD: &str = "ListTasks";

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
    /// Calls the `CancelTask` JSON-RPC method. Returns the task in its
    /// post-cancellation state.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Protocol`] with
    /// [`a2a_types::ErrorCode::TaskNotCancelable`] if the task cannot be
    /// canceled in its current state.
    pub async fn cancel_task(&self, id: impl Into<String>) -> ClientResult<Task> {
        const METHOD: &str = "CancelTask";

        let params = CancelTaskParams {
            tenant: None,
            id: id.into(),
            metadata: None,
        };
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

    /// Subscribes to the SSE stream for an in-progress task.
    ///
    /// Calls the `SubscribeToTask` method. Useful after an unexpected
    /// disconnection from a `SendStreamingMessage` call.
    ///
    /// Events already delivered before the reconnect are **not** replayed.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Protocol`] with
    /// [`a2a_types::ErrorCode::TaskNotFound`] if the task is not in a
    /// streaming-eligible state.
    pub async fn subscribe_to_task(&self, id: impl Into<String>) -> ClientResult<EventStream> {
        const METHOD: &str = "SubscribeToTask";

        let params = TaskIdParams {
            tenant: None,
            id: id.into(),
        };
        let params_value = serde_json::to_value(&params).map_err(ClientError::Serialization)?;

        let mut req = ClientRequest::new(METHOD, params_value);
        self.interceptors.run_before(&mut req).await?;

        self.transport
            .send_streaming_request(METHOD, req.params, &req.extra_headers)
            .await
    }
}
