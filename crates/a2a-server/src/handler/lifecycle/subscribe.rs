// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! `SubscribeToTask` handler — resubscribe to a task's event stream.

use std::collections::HashMap;
use std::time::Instant;

use a2a_protocol_types::params::TaskIdParams;
use a2a_protocol_types::task::TaskId;

use crate::error::{ServerError, ServerResult};
use crate::streaming::InMemoryQueueReader;

use super::super::helpers::build_call_context;
use super::super::RequestHandler;

impl RequestHandler {
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

        let tenant = params.tenant.clone().unwrap_or_default();
        let result: ServerResult<_> = crate::store::tenant::TenantContext::scope(tenant, async {
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
        })
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
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::params::TaskIdParams;

    use crate::agent_executor;
    use crate::builder::RequestHandlerBuilder;
    use crate::error::ServerError;

    struct DummyExecutor;
    agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

    #[tokio::test]
    async fn resubscribe_task_not_found_returns_error() {
        let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
        let params = TaskIdParams {
            tenant: None,
            id: "nonexistent-task".to_owned(),
        };
        let result = handler.on_resubscribe(params, None).await;
        assert!(
            matches!(result, Err(ServerError::TaskNotFound(_))),
            "expected TaskNotFound for missing task, got: {result:?}"
        );
    }
}
