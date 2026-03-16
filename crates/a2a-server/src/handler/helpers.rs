// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Shared helper functions used across handler submodules.

use std::collections::HashMap;

use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::task::Task;

use crate::call_context::CallContext;
use crate::error::{ServerError, ServerResult};

use super::RequestHandler;

/// Validates an ID string: rejects empty/whitespace-only and excessively long values.
pub(super) fn validate_id(raw: &str, name: &str, max_length: usize) -> ServerResult<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ServerError::InvalidParams(format!(
            "{name} must not be empty or whitespace-only"
        )));
    }
    if trimmed.len() > max_length {
        return Err(ServerError::InvalidParams(format!(
            "{name} exceeds maximum length (got {}, max {max_length})",
            trimmed.len()
        )));
    }
    Ok(())
}

/// Builds a [`CallContext`] from a method name and optional HTTP headers.
pub(super) fn build_call_context(
    method: &str,
    headers: Option<&HashMap<String, String>>,
) -> CallContext {
    let mut ctx = CallContext::new(method);
    if let Some(h) = headers {
        ctx.http_headers.clone_from(h);
    }
    ctx
}

impl RequestHandler {
    /// Finds a task by context ID (linear scan for in-memory store).
    pub(crate) async fn find_task_by_context(&self, context_id: &str) -> Option<Task> {
        if context_id.len() > self.limits.max_id_length {
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
}
