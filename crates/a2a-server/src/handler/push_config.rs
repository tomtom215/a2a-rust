// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

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
