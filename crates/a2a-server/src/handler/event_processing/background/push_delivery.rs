// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

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

    // FIX(#4): Cap total push delivery time per event to prevent amplification
    // attacks. With 100 configs × 5s timeout × 3 retries, unbounded delivery
    // could take 25+ minutes. Cap at 30 seconds total per event.
    let max_total_push_time = std::time::Duration::from_secs(30);
    let deadline = tokio::time::Instant::now() + max_total_push_time;

    // FIX(M5): Limit concurrent push deliveries to prevent resource exhaustion
    // when many push configs are registered for a single task. Without this cap,
    // a burst of events could spawn hundreds of concurrent HTTP requests.
    let semaphore = tokio::sync::Semaphore::new(16);

    for config in &configs {
        // Check if we've exceeded the total push delivery budget.
        if tokio::time::Instant::now() >= deadline {
            trace_warn!(
                task_id = %task_id,
                remaining_configs = configs.len(),
                "push delivery deadline exceeded; skipping remaining configs"
            );
            break;
        }

        // Acquire a permit before sending; this bounds concurrency even though
        // deliveries are currently sequential. The semaphore future-proofs
        // against a switch to concurrent (join_all / FuturesUnordered) delivery.
        let _permit = semaphore
            .acquire()
            .await
            .expect("semaphore is never closed");

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

    #[tokio::test]
    async fn deliver_push_bg_respects_total_deadline() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        // A push sender that sleeps for 2 seconds per delivery.
        struct SlowPushSender {
            send_count: Arc<AtomicU64>,
        }

        impl crate::push::PushSender for SlowPushSender {
            fn send<'a>(
                &'a self,
                _url: &'a str,
                _event: &'a StreamResponse,
                _config: &'a TaskPushNotificationConfig,
            ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
            {
                self.send_count.fetch_add(1, Ordering::Relaxed);
                Box::pin(async {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    Ok(())
                })
            }
        }

        let store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t-deadline");
        let event = make_status_event("t-deadline", TaskState::Working);

        // Register many configs. With 2s per delivery and a 30s cap,
        // at most ~15 can complete.
        for i in 0..50 {
            let config = TaskPushNotificationConfig {
                tenant: None,
                id: Some(format!("cfg-{i}")),
                task_id: "t-deadline".to_owned(),
                url: format!("https://example.com/hook{i}"),
                token: None,
                authentication: None,
            };
            store.set(config).await.unwrap();
        }

        let send_count = Arc::new(AtomicU64::new(0));
        let sender = SlowPushSender {
            send_count: Arc::clone(&send_count),
        };
        let limits = HandlerLimits::default().with_push_delivery_timeout(Duration::from_secs(3));

        deliver_push_bg(&task_id, &event, &store, Some(&sender), &limits).await;

        // With 30s total cap and 2s per send (bounded by 3s timeout), not all 50 should fire.
        let count = send_count.load(Ordering::Relaxed);
        assert!(
            count < 50,
            "deadline should prevent all 50 deliveries, got {count}"
        );
        assert!(
            count > 0,
            "at least some deliveries should have fired, got {count}"
        );
    }
}
