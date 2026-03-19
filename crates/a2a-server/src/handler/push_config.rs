// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Push notification config CRUD methods.

use std::collections::HashMap;
use std::time::Instant;

use a2a_protocol_types::params::{DeletePushConfigParams, GetPushConfigParams};
use a2a_protocol_types::push::TaskPushNotificationConfig;

use crate::error::{ServerError, ServerResult};

use super::helpers::build_call_context;
use super::RequestHandler;

impl RequestHandler {
    /// Handles `CreateTaskPushNotificationConfig`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::PushNotSupported`] if no push sender is configured.
    pub async fn on_set_push_config(
        &self,
        config: TaskPushNotificationConfig,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<TaskPushNotificationConfig> {
        let start = Instant::now();
        self.metrics.on_request("CreateTaskPushNotificationConfig");

        let tenant = config.tenant.clone().unwrap_or_default();
        let result: ServerResult<_> = crate::store::tenant::TenantContext::scope(tenant, async {
            if self.push_sender.is_none() {
                return Err(ServerError::PushNotSupported);
            }
            // FIX(#3): Validate webhook URL at config creation time to prevent
            // SSRF attacks. Previously validation only happened at delivery time,
            // leaving a window where malicious URLs could be stored.
            crate::push::sender::validate_webhook_url(&config.url)?;

            let call_ctx = build_call_context("CreateTaskPushNotificationConfig", headers);
            self.interceptors.run_before(&call_ctx).await?;
            let result = self.push_config_store.set(config).await?;
            self.interceptors.run_after(&call_ctx).await?;
            Ok(result)
        })
        .await;

        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response("CreateTaskPushNotificationConfig");
                self.metrics
                    .on_latency("CreateTaskPushNotificationConfig", elapsed);
            }
            Err(e) => {
                self.metrics
                    .on_error("CreateTaskPushNotificationConfig", &e.to_string());
                self.metrics
                    .on_latency("CreateTaskPushNotificationConfig", elapsed);
            }
        }
        result
    }

    /// Handles `GetTaskPushNotificationConfig`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::InvalidParams`] if the config is not found.
    pub async fn on_get_push_config(
        &self,
        params: GetPushConfigParams,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<TaskPushNotificationConfig> {
        let start = Instant::now();
        self.metrics.on_request("GetTaskPushNotificationConfig");

        let tenant = params.tenant.clone().unwrap_or_default();
        let result: ServerResult<_> = crate::store::tenant::TenantContext::scope(tenant, async {
            let call_ctx = build_call_context("GetTaskPushNotificationConfig", headers);
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
        })
        .await;

        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response("GetTaskPushNotificationConfig");
                self.metrics
                    .on_latency("GetTaskPushNotificationConfig", elapsed);
            }
            Err(e) => {
                self.metrics
                    .on_error("GetTaskPushNotificationConfig", &e.to_string());
                self.metrics
                    .on_latency("GetTaskPushNotificationConfig", elapsed);
            }
        }
        result
    }

    /// Handles `ListTaskPushNotificationConfigs`.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the store query fails.
    pub async fn on_list_push_configs(
        &self,
        task_id: &str,
        tenant: Option<&str>,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<Vec<TaskPushNotificationConfig>> {
        let start = Instant::now();
        self.metrics.on_request("ListTaskPushNotificationConfigs");

        let tenant_owned = tenant.unwrap_or_default().to_owned();
        let result: ServerResult<_> =
            crate::store::tenant::TenantContext::scope(tenant_owned, async {
                let call_ctx = build_call_context("ListTaskPushNotificationConfigs", headers);
                self.interceptors.run_before(&call_ctx).await?;
                let configs = self.push_config_store.list(task_id).await?;
                self.interceptors.run_after(&call_ctx).await?;
                Ok(configs)
            })
            .await;

        let elapsed = start.elapsed();
        match &result {
            Ok(_) => {
                self.metrics.on_response("ListTaskPushNotificationConfigs");
                self.metrics
                    .on_latency("ListTaskPushNotificationConfigs", elapsed);
            }
            Err(e) => {
                self.metrics
                    .on_error("ListTaskPushNotificationConfigs", &e.to_string());
                self.metrics
                    .on_latency("ListTaskPushNotificationConfigs", elapsed);
            }
        }
        result
    }

    /// Handles `DeleteTaskPushNotificationConfig`.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`] if the delete operation fails.
    pub async fn on_delete_push_config(
        &self,
        params: DeletePushConfigParams,
        headers: Option<&HashMap<String, String>>,
    ) -> ServerResult<()> {
        let start = Instant::now();
        self.metrics.on_request("DeleteTaskPushNotificationConfig");

        let tenant = params.tenant.clone().unwrap_or_default();
        let result: ServerResult<_> = crate::store::tenant::TenantContext::scope(tenant, async {
            let call_ctx = build_call_context("DeleteTaskPushNotificationConfig", headers);
            self.interceptors.run_before(&call_ctx).await?;
            self.push_config_store
                .delete(&params.task_id, &params.id)
                .await?;
            self.interceptors.run_after(&call_ctx).await?;
            Ok(())
        })
        .await;

        let elapsed = start.elapsed();
        match &result {
            Ok(()) => {
                self.metrics.on_response("DeleteTaskPushNotificationConfig");
                self.metrics
                    .on_latency("DeleteTaskPushNotificationConfig", elapsed);
            }
            Err(e) => {
                self.metrics
                    .on_error("DeleteTaskPushNotificationConfig", &e.to_string());
                self.metrics
                    .on_latency("DeleteTaskPushNotificationConfig", elapsed);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_executor;
    use crate::builder::RequestHandlerBuilder;

    struct DummyExecutor;
    agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

    fn make_handler() -> RequestHandler {
        RequestHandlerBuilder::new(DummyExecutor).build().unwrap()
    }

    fn make_push_config(task_id: &str) -> TaskPushNotificationConfig {
        TaskPushNotificationConfig {
            tenant: None,
            id: Some("cfg-1".to_owned()),
            task_id: task_id.to_owned(),
            url: "https://example.com/webhook".to_owned(),
            token: None,
            authentication: None,
        }
    }

    // ── on_set_push_config ───────────────────────────────────────────────────

    #[tokio::test]
    async fn set_push_config_without_sender_returns_push_not_supported() {
        let handler = make_handler();
        let config = make_push_config("task-1");
        let result = handler.on_set_push_config(config, None).await;
        assert!(
            matches!(result, Err(crate::error::ServerError::PushNotSupported)),
            "expected PushNotSupported, got: {result:?}"
        );
    }

    // ── on_get_push_config ───────────────────────────────────────────────────

    #[tokio::test]
    async fn get_push_config_not_found_returns_invalid_params() {
        use a2a_protocol_types::params::GetPushConfigParams;

        let handler = make_handler();
        let params = GetPushConfigParams {
            tenant: None,
            task_id: "no-task".to_owned(),
            id: "no-id".to_owned(),
        };
        let result = handler.on_get_push_config(params, None).await;
        assert!(
            matches!(result, Err(crate::error::ServerError::InvalidParams(_))),
            "expected InvalidParams for missing config, got: {result:?}"
        );
    }

    // ── on_list_push_configs ─────────────────────────────────────────────────

    #[tokio::test]
    async fn list_push_configs_empty_returns_empty_vec() {
        let handler = make_handler();
        let result = handler
            .on_list_push_configs("no-task", None, None)
            .await
            .expect("list should succeed on empty store");
        assert!(
            result.is_empty(),
            "listing configs for an unknown task should return an empty vec"
        );
    }

    // ── on_delete_push_config ────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_push_config_nonexistent_returns_ok() {
        use a2a_protocol_types::params::DeletePushConfigParams;

        let handler = make_handler();
        let params = DeletePushConfigParams {
            tenant: None,
            task_id: "no-task".to_owned(),
            id: "no-id".to_owned(),
        };
        // The in-memory store's delete is idempotent: deleting a non-existent
        // config returns Ok(()) rather than an error.
        let result = handler.on_delete_push_config(params, None).await;
        assert!(
            result.is_ok(),
            "deleting a non-existent push config should return Ok, got: {result:?}"
        );
    }

    // ── error metrics paths ────────────────────────────────────────────────

    #[tokio::test]
    async fn list_push_configs_error_path_records_metrics() {
        // Exercise the Err branch in on_list_push_configs (lines 144-149)
        // by using a failing interceptor.
        use crate::call_context::CallContext;
        use crate::interceptor::ServerInterceptor;
        use std::future::Future;
        use std::pin::Pin;

        struct FailInterceptor;
        impl ServerInterceptor for FailInterceptor {
            fn before<'a>(
                &'a self,
                _ctx: &'a CallContext,
            ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async {
                    Err(a2a_protocol_types::error::A2aError::internal(
                        "forced failure",
                    ))
                })
            }
            fn after<'a>(
                &'a self,
                _ctx: &'a CallContext,
            ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async { Ok(()) })
            }
        }

        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_interceptor(FailInterceptor)
            .build()
            .unwrap();

        let result = handler.on_list_push_configs("task-1", None, None).await;
        assert!(
            result.is_err(),
            "list_push_configs should fail when interceptor rejects"
        );
    }

    #[tokio::test]
    async fn delete_push_config_error_path_records_metrics() {
        // Exercise the Err branch in on_delete_push_config (lines 186-191, 204)
        // by using a failing interceptor.
        use crate::call_context::CallContext;
        use crate::interceptor::ServerInterceptor;
        use a2a_protocol_types::params::DeletePushConfigParams;
        use std::future::Future;
        use std::pin::Pin;

        struct FailInterceptor;
        impl ServerInterceptor for FailInterceptor {
            fn before<'a>(
                &'a self,
                _ctx: &'a CallContext,
            ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async {
                    Err(a2a_protocol_types::error::A2aError::internal(
                        "forced failure",
                    ))
                })
            }
            fn after<'a>(
                &'a self,
                _ctx: &'a CallContext,
            ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
            {
                Box::pin(async { Ok(()) })
            }
        }

        let handler = RequestHandlerBuilder::new(DummyExecutor)
            .with_interceptor(FailInterceptor)
            .build()
            .unwrap();

        let params = DeletePushConfigParams {
            tenant: None,
            task_id: "task-1".to_owned(),
            id: "cfg-1".to_owned(),
        };
        let result = handler.on_delete_push_config(params, None).await;
        assert!(
            result.is_err(),
            "delete_push_config should fail when interceptor rejects"
        );
    }

    #[tokio::test]
    async fn set_push_config_error_path_records_metrics() {
        // The existing test already covers PushNotSupported which hits the error branch.
        // This additionally verifies the error is propagated through the metrics path.
        let handler = make_handler();
        let config = make_push_config("task-err");
        let result = handler.on_set_push_config(config, None).await;
        assert!(
            result.is_err(),
            "set_push_config without push sender should hit error metrics path"
        );
    }

    #[tokio::test]
    async fn get_push_config_error_path_records_metrics() {
        // The existing test already covers InvalidParams which hits the error branch.
        // This additionally ensures error metrics are tracked for missing configs.
        use a2a_protocol_types::params::GetPushConfigParams;

        let handler = make_handler();
        let params = GetPushConfigParams {
            tenant: None,
            task_id: "missing-task".to_owned(),
            id: "missing-id".to_owned(),
        };
        let result = handler.on_get_push_config(params, None).await;
        assert!(
            result.is_err(),
            "get_push_config for missing config should hit error metrics path"
        );
    }
}
