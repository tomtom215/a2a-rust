// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification delivery for background event processing.
//!
//! Delivers push notifications to configured webhook endpoints when
//! streaming events occur, with timeout enforcement.

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::TaskId;

use crate::handler::limits::HandlerLimits;
use crate::push::{PushConfigStore, PushSender};

/// Delivers push notifications for a streaming event to all configured endpoints.
///
/// Silently swallows errors from the push config store and logs warnings for
/// delivery failures/timeouts — background push delivery must never block or
/// crash the event processing loop.
pub(super) async fn deliver_push_bg(
    task_id: &TaskId,
    event: &StreamResponse,
    push_config_store: &dyn PushConfigStore,
    push_sender: Option<&dyn PushSender>,
    limits: &HandlerLimits,
) {
    let Some(sender) = push_sender else {
        return;
    };
    let Ok(configs) = push_config_store.list(task_id.as_ref()).await else {
        return;
    };
    for config in &configs {
        let result = tokio::time::timeout(
            limits.push_delivery_timeout,
            sender.send(&config.url, event, config),
        )
        .await;
        match result {
            Ok(Err(_err)) => {
                trace_warn!(
                    task_id = %task_id,
                    url = %config.url,
                    error = %_err,
                    "push notification delivery failed (background)"
                );
            }
            Err(_) => {
                trace_warn!(
                    task_id = %task_id,
                    url = %config.url,
                    "push notification delivery timed out (background)"
                );
            }
            Ok(Ok(())) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use a2a_protocol_types::error::A2aError;
    use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
    use a2a_protocol_types::push::TaskPushNotificationConfig;
    use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

    use crate::handler::limits::HandlerLimits;
    use crate::push::{InMemoryPushConfigStore, PushConfigStore};

    use super::*;

    /// A push config store that always returns errors.
    struct AlwaysErrPushConfigStore;

    impl PushConfigStore for AlwaysErrPushConfigStore {
        fn set<'a>(
            &'a self,
            _cfg: TaskPushNotificationConfig,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = a2a_protocol_types::error::A2aResult<TaskPushNotificationConfig>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Err(A2aError::internal("always err")) })
        }
        fn get<'a>(
            &'a self,
            _task_id: &'a str,
            _id: &'a str,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = a2a_protocol_types::error::A2aResult<
                            Option<TaskPushNotificationConfig>,
                        >,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Err(A2aError::internal("always err")) })
        }
        fn list<'a>(
            &'a self,
            _task_id: &'a str,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = a2a_protocol_types::error::A2aResult<
                            Vec<TaskPushNotificationConfig>,
                        >,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Err(A2aError::internal("always err")) })
        }
        fn delete<'a>(
            &'a self,
            _task_id: &'a str,
            _id: &'a str,
        ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async { Err(A2aError::internal("always err")) })
        }
    }

    fn make_status_event(task_id: &str, state: TaskState) -> StreamResponse {
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new(task_id),
            context_id: ContextId::new("ctx-1"),
            status: TaskStatus::new(state),
            metadata: None,
        })
    }

    fn default_limits() -> HandlerLimits {
        HandlerLimits::default()
    }

    #[tokio::test]
    async fn deliver_push_bg_with_no_sender_is_noop() {
        let store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t1");
        let event = make_status_event("t1", TaskState::Working);

        deliver_push_bg(&task_id, &event, &store, None, &default_limits()).await;
    }

    #[tokio::test]
    async fn deliver_push_bg_with_failing_store_returns_silently() {
        let store = AlwaysErrPushConfigStore;
        let task_id = TaskId::new("t1");
        let event = make_status_event("t1", TaskState::Working);

        deliver_push_bg(&task_id, &event, &store, None, &default_limits()).await;
    }
}
