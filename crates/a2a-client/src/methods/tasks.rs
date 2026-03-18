// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task management client methods.
//!
//! Provides `get_task`, `list_tasks`, `cancel_task`, and `subscribe_to_task`
//! on [`A2aClient`].

use a2a_protocol_types::{
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
    /// Returns [`ClientError::Protocol`] with [`a2a_protocol_types::ErrorCode::TaskNotFound`]
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
            result,
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<Task>(resp.result).map_err(ClientError::Serialization)
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
            result,
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<TaskListResponse>(resp.result).map_err(ClientError::Serialization)
    }

    /// Requests cancellation of a running task.
    ///
    /// Calls the `CancelTask` JSON-RPC method. Returns the task in its
    /// post-cancellation state.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Protocol`] with
    /// [`a2a_protocol_types::ErrorCode::TaskNotCancelable`] if the task cannot be
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
            result,
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        serde_json::from_value::<Task>(resp.result).map_err(ClientError::Serialization)
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
    /// [`a2a_protocol_types::ErrorCode::TaskNotFound`] if the task is not in a
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

        let stream = self
            .transport
            .send_streaming_request(METHOD, req.params, &req.extra_headers)
            .await?;

        // FIX(#6): Call run_after() for streaming requests so interceptors
        // get their cleanup/logging hook.
        let resp = ClientResponse {
            method: METHOD.to_owned(),
            result: serde_json::Value::Null,
            status_code: 200,
        };
        self.interceptors.run_after(&resp).await?;

        Ok(stream)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;

    use a2a_protocol_types::{ListTasksParams, TaskQueryParams};

    use crate::error::{ClientError, ClientResult};
    use crate::streaming::EventStream;
    use crate::transport::Transport;
    use crate::ClientBuilder;

    /// A mock transport that returns a pre-configured JSON value for requests
    /// and an error for streaming requests.
    struct MockTransport {
        response: serde_json::Value,
    }

    impl MockTransport {
        fn new(response: serde_json::Value) -> Self {
            Self { response }
        }
    }

    impl Transport for MockTransport {
        fn send_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
            let resp = self.response.clone();
            Box::pin(async move { Ok(resp) })
        }

        fn send_streaming_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
            Box::pin(async move {
                Err(ClientError::Transport(
                    "mock: streaming not supported".into(),
                ))
            })
        }
    }

    fn make_client(transport: impl Transport) -> crate::A2aClient {
        ClientBuilder::new("http://localhost:8080")
            .with_custom_transport(transport)
            .build()
            .expect("build client")
    }

    fn task_json() -> serde_json::Value {
        serde_json::json!({
            "id": "task-1",
            "contextId": "ctx-1",
            "status": {
                "state": "TASK_STATE_COMPLETED"
            }
        })
    }

    #[tokio::test]
    async fn get_task_success() {
        let transport = MockTransport::new(task_json());
        let client = make_client(transport);

        let params = TaskQueryParams {
            tenant: None,
            id: "task-1".into(),
            history_length: None,
        };
        let task = client.get_task(params).await.unwrap();
        assert_eq!(task.id.as_ref(), "task-1");
    }

    #[tokio::test]
    async fn list_tasks_success() {
        let response = serde_json::json!({
            "tasks": [
                {
                    "id": "task-1",
                    "contextId": "ctx-1",
                    "status": { "state": "TASK_STATE_COMPLETED" }
                },
                {
                    "id": "task-2",
                    "contextId": "ctx-2",
                    "status": { "state": "TASK_STATE_WORKING" }
                }
            ]
        });
        let transport = MockTransport::new(response);
        let client = make_client(transport);

        let params = ListTasksParams::default();
        let result = client.list_tasks(params).await.unwrap();
        assert_eq!(result.tasks.len(), 2);
        assert_eq!(result.tasks[0].id.as_ref(), "task-1");
    }

    #[tokio::test]
    async fn cancel_task_success() {
        let transport = MockTransport::new(task_json());
        let client = make_client(transport);

        let task = client.cancel_task("task-1").await.unwrap();
        assert_eq!(task.id.as_ref(), "task-1");
    }

    #[tokio::test]
    async fn subscribe_to_task_returns_transport_error() {
        // MockTransport returns an error for streaming requests, exercising
        // the subscribe_to_task code path through param serialization and
        // interceptor invocation before hitting the transport.
        let transport = MockTransport::new(serde_json::Value::Null);
        let client = make_client(transport);

        let err = client.subscribe_to_task("task-1").await.unwrap_err();
        assert!(
            matches!(err, ClientError::Transport(ref msg) if msg.contains("streaming not supported")),
            "expected Transport error, got {err:?}"
        );
    }
}
