// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! `GetTask` handler — retrieves a single task by ID.

use std::collections::HashMap;
use std::time::Instant;

use a2a_protocol_types::params::TaskQueryParams;
use a2a_protocol_types::task::{Task, TaskId};

use crate::error::{ServerError, ServerResult};

use super::super::helpers::build_call_context;
use super::super::RequestHandler;

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

        let tenant = params.tenant.clone().unwrap_or_default();
        let result: ServerResult<_> = crate::store::tenant::TenantContext::scope(tenant, async {
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
        })
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
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::params::TaskQueryParams;
    use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

    use crate::agent_executor;
    use crate::builder::RequestHandlerBuilder;
    use crate::error::ServerError;

    struct DummyExecutor;
    agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

    fn make_completed_task(id: &str) -> Task {
        Task {
            id: TaskId::new(id),
            context_id: ContextId::new("ctx-1"),
            status: TaskStatus::new(TaskState::Completed),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    #[tokio::test]
    async fn get_task_not_found_returns_error() {
        let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
        let params = TaskQueryParams {
            tenant: None,
            id: "nonexistent-task".to_owned(),
            history_length: None,
        };
        let result = handler.on_get_task(params, None).await;
        assert!(
            matches!(result, Err(ServerError::TaskNotFound(_))),
            "expected TaskNotFound for missing task, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn get_task_found_returns_task() {
        let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
        let task = make_completed_task("t-get-1");
        handler.task_store.save(task).await.unwrap();

        let params = TaskQueryParams {
            tenant: None,
            id: "t-get-1".to_owned(),
            history_length: None,
        };
        let result = handler.on_get_task(params, None).await;
        assert!(
            result.is_ok(),
            "expected Ok for existing task, got: {result:?}"
        );
        assert_eq!(result.unwrap().id, TaskId::new("t-get-1"));
    }
}
