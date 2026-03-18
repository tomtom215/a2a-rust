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
        ctx = ctx.with_http_headers(h.clone());
    }
    ctx
}

impl RequestHandler {
    /// Finds a task by context ID, scoped to the current tenant.
    ///
    /// Uses [`crate::store::tenant::TenantContext::current()`] so that
    /// multi-tenant deployments only search within the caller's tenant.
    pub(crate) async fn find_task_by_context(&self, context_id: &str) -> Option<Task> {
        if context_id.len() > self.limits.max_id_length {
            return None;
        }
        // Use the current tenant context so multi-tenant stores scope correctly.
        let tenant = crate::store::tenant::TenantContext::current();
        let tenant_param = if tenant.is_empty() {
            None
        } else {
            Some(tenant)
        };
        let params = ListTasksParams {
            tenant: tenant_param,
            context_id: Some(context_id.to_owned()),
            status: None,
            page_size: Some(1),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        };
        match self.task_store.list(&params).await {
            Ok(resp) => resp.tasks.into_iter().next(),
            Err(_e) => {
                trace_error!(context_id, error = %_e, "find_task_by_context: store query failed");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_id ────────────────────────────────────────────────────────

    #[test]
    fn validate_id_accepts_normal_id() {
        assert!(
            validate_id("task-123", "task_id", 1024).is_ok(),
            "a normal short ID should be accepted"
        );
    }

    #[test]
    fn validate_id_rejects_empty_string() {
        let err = validate_id("", "task_id", 1024).unwrap_err();
        assert!(
            matches!(err, ServerError::InvalidParams(ref msg) if msg.contains("empty")),
            "empty string should be rejected with InvalidParams: {err:?}"
        );
    }

    #[test]
    fn validate_id_rejects_whitespace_only() {
        let err = validate_id("   \t\n  ", "context_id", 1024).unwrap_err();
        assert!(
            matches!(err, ServerError::InvalidParams(ref msg) if msg.contains("empty")),
            "whitespace-only string should be rejected: {err:?}"
        );
    }

    #[test]
    fn validate_id_rejects_exceeding_max_length() {
        let long_id = "a".repeat(2000);
        let err = validate_id(&long_id, "task_id", 1024).unwrap_err();
        assert!(
            matches!(err, ServerError::InvalidParams(ref msg) if msg.contains("maximum length")),
            "overly long ID should be rejected: {err:?}"
        );
    }

    #[test]
    fn validate_id_accepts_exactly_max_length() {
        let exact = "b".repeat(128);
        assert!(
            validate_id(&exact, "task_id", 128).is_ok(),
            "ID at exactly max length should be accepted"
        );
    }

    #[test]
    fn validate_id_trims_before_length_check() {
        // 3 content chars + surrounding whitespace = 7 raw chars, but trimmed = 3
        assert!(
            validate_id("  abc  ", "id", 3).is_ok(),
            "trimmed length (3) should pass a max of 3"
        );
    }

    #[test]
    fn validate_id_includes_field_name_in_error() {
        let err = validate_id("", "my_field", 1024).unwrap_err();
        assert!(
            matches!(err, ServerError::InvalidParams(ref msg) if msg.contains("my_field")),
            "error message should contain the field name: {err:?}"
        );
    }

    // ── build_call_context ─────────────────────────────────────────────────

    #[test]
    fn build_call_context_without_headers() {
        let ctx = build_call_context("message/send", None);
        assert_eq!(ctx.method(), "message/send", "method should be set");
        assert!(
            ctx.http_headers().is_empty(),
            "headers should be empty when None is passed"
        );
    }

    #[test]
    fn build_call_context_with_headers() {
        let mut headers = HashMap::new();
        headers.insert("authorization".to_owned(), "Bearer tok".to_owned());
        headers.insert("x-request-id".to_owned(), "req-99".to_owned());

        let ctx = build_call_context("tasks/get", Some(&headers));
        assert_eq!(ctx.method(), "tasks/get");
        assert_eq!(
            ctx.http_headers().get("authorization").map(String::as_str),
            Some("Bearer tok"),
            "headers should be cloned into the context"
        );
        assert_eq!(
            ctx.http_headers().get("x-request-id").map(String::as_str),
            Some("req-99"),
        );
    }

    #[test]
    fn build_call_context_with_empty_headers_map() {
        let headers = HashMap::new();
        let ctx = build_call_context("test", Some(&headers));
        assert!(
            ctx.http_headers().is_empty(),
            "an empty map should result in empty headers"
        );
    }

    // ── find_task_by_context ─────────────────────────────────────────────

    mod find_task_by_context_tests {
        use crate::agent_executor;
        use crate::builder::RequestHandlerBuilder;
        use crate::handler::limits::HandlerLimits;

        struct DummyExecutor;
        agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

        #[tokio::test]
        async fn context_id_too_long_returns_none() {
            // Covers line 49: early return None when context_id exceeds max_id_length.
            let handler = RequestHandlerBuilder::new(DummyExecutor)
                .with_handler_limits(HandlerLimits::default().with_max_id_length(10))
                .build()
                .unwrap();

            let long_id = "a".repeat(11);
            let result = handler.find_task_by_context(&long_id).await;
            assert!(
                result.is_none(),
                "find_task_by_context should return None for context_id exceeding max length"
            );
        }

        #[tokio::test]
        async fn context_id_within_limit_returns_none_for_missing() {
            let handler = RequestHandlerBuilder::new(DummyExecutor)
                .with_handler_limits(HandlerLimits::default().with_max_id_length(100))
                .build()
                .unwrap();

            let result = handler.find_task_by_context("no-such-context").await;
            assert!(
                result.is_none(),
                "find_task_by_context should return None when no task matches the context"
            );
        }
    }
}
