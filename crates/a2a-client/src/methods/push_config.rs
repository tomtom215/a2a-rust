// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification configuration client methods.
//!
//! Provides `set_push_config`, `get_push_config`, `list_push_configs`, and
//! `delete_push_config` on [`A2aClient`].

use a2a_types::{DeletePushConfigParams, GetPushConfigParams, TaskId, TaskPushNotificationConfig};

use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};
use crate::interceptor::{ClientRequest, ClientResponse};

impl A2aClient {
    /// Registers or replaces a push notification configuration for a task.
    ///
    /// Calls `tasks/pushNotificationConfig/set`. Returns the configuration as
    /// stored by the server (including the server-assigned `id`).
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Protocol`] with
    /// [`a2a_types::ErrorCode::PushNotificationNotSupported`] if the agent
    /// does not support push notifications.
    pub async fn set_push_config(
        &self,
        config: TaskPushNotificationConfig,
    ) -> ClientResult<TaskPushNotificationConfig> {
        const METHOD: &str = "tasks/pushNotificationConfig/set";

        let params_value = serde_json::to_value(&config).map_err(ClientError::Serialization)?;

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

        serde_json::from_value::<TaskPushNotificationConfig>(result)
            .map_err(ClientError::Serialization)
    }

    /// Retrieves a push notification configuration by task ID and config ID.
    ///
    /// Calls `tasks/pushNotificationConfig/get`.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or protocol errors.
    pub async fn get_push_config(
        &self,
        task_id: TaskId,
        id: impl Into<String>,
    ) -> ClientResult<TaskPushNotificationConfig> {
        const METHOD: &str = "tasks/pushNotificationConfig/get";

        let params = GetPushConfigParams {
            task_id,
            id: id.into(),
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

        serde_json::from_value::<TaskPushNotificationConfig>(result)
            .map_err(ClientError::Serialization)
    }

    /// Lists all push notification configurations for a task.
    ///
    /// Calls `tasks/pushNotificationConfig/list`.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or protocol errors.
    pub async fn list_push_configs(
        &self,
        task_id: TaskId,
    ) -> ClientResult<Vec<TaskPushNotificationConfig>> {
        const METHOD: &str = "tasks/pushNotificationConfig/list";

        let params = serde_json::json!({ "taskId": task_id });
        let mut req = ClientRequest::new(METHOD, params);
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

        serde_json::from_value::<Vec<TaskPushNotificationConfig>>(result)
            .map_err(ClientError::Serialization)
    }

    /// Deletes a push notification configuration.
    ///
    /// Calls `tasks/pushNotificationConfig/delete`.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or protocol errors.
    pub async fn delete_push_config(
        &self,
        task_id: TaskId,
        id: impl Into<String>,
    ) -> ClientResult<()> {
        const METHOD: &str = "tasks/pushNotificationConfig/delete";

        let params = DeletePushConfigParams {
            task_id,
            id: id.into(),
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

        Ok(())
    }
}
