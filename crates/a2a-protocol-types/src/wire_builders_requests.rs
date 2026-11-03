// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Constructors and builders for the request parameters and the paged
//! responses, which are `#[non_exhaustive]` (audit G-10, the 26 wire structs).
//!
//! `new` takes the fields the protocol requires; a `with_*` method sets each
//! optional one. Each leaves every field it is not given at the value a struct
//! literal would have written (`None`, or empty), so converting a literal
//! changes no wire byte. [`MessageSendParams`] and [`TaskQueryParams`] had
//! theirs already (`params/builders.rs`).
//!
//! [`MessageSendParams`]: crate::params::MessageSendParams
//! [`TaskQueryParams`]: crate::params::TaskQueryParams

use serde_json::Value;

use crate::params::{
    CancelTaskParams, DeletePushConfigParams, GetExtendedAgentCardParams, GetPushConfigParams,
    ListPushConfigsParams, ListTasksParams, SendMessageConfiguration, TaskIdParams,
};
use crate::push::TaskPushNotificationConfig;
use crate::responses::{ListPushConfigsResponse, TaskListResponse};
use crate::task::TaskState;

impl SendMessageConfiguration {
    /// The default configuration: plain text accepted, no history limit, no
    /// push configuration, and the call waits for the task.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the media types the client accepts in the agent's reply.
    #[must_use]
    pub fn with_accepted_output_modes(mut self, modes: Vec<String>) -> Self {
        self.accepted_output_modes = modes;
        self
    }

    /// Asks the server to push the task's updates to `config`.
    #[must_use]
    pub fn with_task_push_notification_config(
        mut self,
        config: TaskPushNotificationConfig,
    ) -> Self {
        self.task_push_notification_config = Some(config);
        self
    }

    /// Asks for at most `n` of the most recent history messages.
    #[must_use]
    pub const fn with_history_length(mut self, n: u32) -> Self {
        self.history_length = Some(n);
        self
    }

    /// Sets whether the call returns before the task completes.
    #[must_use]
    pub const fn with_return_immediately(mut self, return_immediately: bool) -> Self {
        self.return_immediately = Some(return_immediately);
        self
    }
}

impl ListTasksParams {
    /// A listing with no filter, no page size and no page token.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Scopes the request to `tenant`.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }

    /// Lists only the tasks of `context_id`.
    #[must_use]
    pub fn with_context_id(mut self, context_id: impl Into<String>) -> Self {
        self.context_id = Some(context_id.into());
        self
    }

    /// Lists only the tasks in `state`.
    #[must_use]
    pub const fn with_status(mut self, state: TaskState) -> Self {
        self.status = Some(state);
        self
    }

    /// Asks for at most `n` tasks in the page.
    #[must_use]
    pub const fn with_page_size(mut self, n: u32) -> Self {
        self.page_size = Some(n);
        self
    }

    /// Continues from the `next_page_token` of an earlier page.
    #[must_use]
    pub fn with_page_token(mut self, token: impl Into<String>) -> Self {
        self.page_token = Some(token.into());
        self
    }

    /// Asks for at most `n` of the most recent history messages per task.
    #[must_use]
    pub const fn with_history_length(mut self, n: u32) -> Self {
        self.history_length = Some(n);
        self
    }

    /// Lists only the tasks whose status changed after `timestamp` (ISO 8601).
    #[must_use]
    pub fn with_status_timestamp_after(mut self, timestamp: impl Into<String>) -> Self {
        self.status_timestamp_after = Some(timestamp.into());
        self
    }

    /// Includes each task's artifacts in the answer.
    #[must_use]
    pub const fn with_include_artifacts(mut self, include: bool) -> Self {
        self.include_artifacts = Some(include);
        self
    }
}

impl CancelTaskParams {
    /// Parameters for cancelling task `id`, with no tenant and no metadata.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            tenant: None,
            id: id.into(),
            metadata: None,
        }
    }

    /// Scopes the request to `tenant`.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }

    /// Attaches opaque metadata to the cancellation request.
    #[must_use]
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl GetPushConfigParams {
    /// Parameters for reading push configuration `id` of task `task_id`.
    #[must_use]
    pub fn new(task_id: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            tenant: None,
            task_id: task_id.into(),
            id: id.into(),
        }
    }

    /// Scopes the request to `tenant`.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }
}

impl DeletePushConfigParams {
    /// Parameters for deleting push configuration `id` of task `task_id`.
    #[must_use]
    pub fn new(task_id: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            tenant: None,
            task_id: task_id.into(),
            id: id.into(),
        }
    }

    /// Scopes the request to `tenant`.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }
}

impl TaskIdParams {
    /// Parameters for a request that names only task `id` (subscribe, for one).
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            tenant: None,
            id: id.into(),
        }
    }

    /// Scopes the request to `tenant`.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }
}

impl ListPushConfigsParams {
    /// Parameters for listing the push configurations of task `task_id`.
    #[must_use]
    pub fn new(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            page_size: None,
            page_token: None,
            tenant: None,
        }
    }

    /// Asks for at most `n` configurations in the page.
    #[must_use]
    pub const fn with_page_size(mut self, n: u32) -> Self {
        self.page_size = Some(n);
        self
    }

    /// Continues from the `next_page_token` of an earlier page.
    #[must_use]
    pub fn with_page_token(mut self, token: impl Into<String>) -> Self {
        self.page_token = Some(token.into());
        self
    }

    /// Scopes the request to `tenant`.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }
}

impl GetExtendedAgentCardParams {
    /// Parameters for fetching the authenticated extended card, with no tenant.
    #[must_use]
    pub const fn new() -> Self {
        Self { tenant: None }
    }

    /// Scopes the request to `tenant`.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }
}

impl Default for GetExtendedAgentCardParams {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskListResponse {
    /// Sets the token for the next page. [`TaskListResponse::new`] leaves it
    /// empty, which the protocol reads as "this is the last page".
    #[must_use]
    pub fn with_next_page_token(mut self, token: impl Into<String>) -> Self {
        self.next_page_token = token.into();
        self
    }

    /// Sets the number of tasks in this page, where [`TaskListResponse::new`]
    /// uses the length of the list it was given.
    #[must_use]
    pub const fn with_page_size(mut self, n: u32) -> Self {
        self.page_size = n;
        self
    }

    /// Sets the total number of matching tasks, where [`TaskListResponse::new`]
    /// uses the length of the list it was given.
    #[must_use]
    pub const fn with_total_size(mut self, n: u32) -> Self {
        self.total_size = n;
        self
    }
}

impl ListPushConfigsResponse {
    /// A page of push configurations with no next page.
    #[must_use]
    pub const fn new(configs: Vec<TaskPushNotificationConfig>) -> Self {
        Self {
            configs,
            next_page_token: None,
        }
    }

    /// Sets the token for the next page.
    #[must_use]
    pub fn with_next_page_token(mut self, token: impl Into<String>) -> Self {
        self.next_page_token = Some(token.into());
        self
    }
}

#[cfg(test)]
#[path = "wire_builders_requests_tests.rs"]
mod tests;
