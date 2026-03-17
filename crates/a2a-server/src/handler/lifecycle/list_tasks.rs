// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! `ListTasks` handler — paginated task listing with filters.

use std::collections::HashMap;
use std::time::Instant;

use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;

use crate::error::ServerResult;

use super::super::helpers::build_call_context;
use super::super::RequestHandler;

impl RequestHandler {
    /// Handles `ListTasks`.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`](crate::error::ServerError) if the store query fails.
    pub async fn on_list_tasks(
        &self,
        params: ListTasksParams,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<TaskListResponse> {
        let start = Instant::now();
        trace_info!(method = "ListTasks", "handling list tasks");
        self.metrics.on_request("ListTasks");

        let tenant = params.tenant.clone().unwrap_or_default();
        let result: ServerResult<_> = crate::store::tenant::TenantContext::scope(tenant, async {
            let call_ctx = build_call_context("ListTasks", headers);
            self.interceptors.run_before(&call_ctx).await?;
            let result = self.task_store.list(&params).await?;
            self.interceptors.run_after(&call_ctx).await?;
            Ok(result)
        })
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
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::params::ListTasksParams;
    use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

    use crate::agent_executor;
    use crate::builder::RequestHandlerBuilder;

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
    async fn list_tasks_empty_store_returns_empty() {
        let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
        let params = ListTasksParams::default();
        let result = handler
            .on_list_tasks(params, None)
            .await
            .expect("list_tasks should succeed on empty store");
        assert!(
            result.tasks.is_empty(),
            "listing tasks on an empty store should return an empty list"
        );
    }

    #[tokio::test]
    async fn list_tasks_returns_saved_task() {
        let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
        let task = make_completed_task("t-list-1");
        handler.task_store.save(task).await.unwrap();

        let params = ListTasksParams::default();
        let result = handler
            .on_list_tasks(params, None)
            .await
            .expect("list_tasks should succeed");
        assert_eq!(result.tasks.len(), 1, "should return the one saved task");
    }
}
