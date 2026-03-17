// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Background event processing for streaming mode.

use std::sync::Arc;

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

use crate::push::{PushConfigStore, PushSender};
use crate::store::TaskStore;
use crate::streaming::EventQueueReader;

use super::super::limits::HandlerLimits;
use super::super::RequestHandler;

// ── Background event processor (streaming mode) ─────────────────────────────

impl RequestHandler {
    /// Spawns a background task that subscribes to the event queue and
    /// processes events (state transitions, task store updates, push delivery).
    ///
    /// This is the architectural fix for push delivery in streaming mode:
    /// previously, `deliver_push()` was only called from `collect_events()`
    /// which only runs for sync (non-streaming) mode. This background
    /// processor ensures push notifications fire for every event regardless
    /// of whether the consumer is streaming or synchronous.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn spawn_background_event_processor(
        &self,
        task_id: TaskId,
        executor_handle: tokio::task::JoinHandle<()>,
    ) {
        let task_store = Arc::clone(&self.task_store);
        let push_config_store = Arc::clone(&self.push_config_store);
        let push_sender = self.push_sender.clone();
        let limits = self.limits.clone();

        // Subscribe a second reader from the broadcast channel.
        // The SSE reader and this background reader both see every event.
        let event_queue_mgr = self.event_queue_manager.clone();

        // Capture the current tenant context so background store operations
        // are scoped to the correct tenant (task_local doesn't propagate
        // across tokio::spawn).
        let tenant = crate::store::tenant::TenantContext::current();

        tokio::spawn(crate::store::tenant::TenantContext::scope(
            tenant,
            async move {
                // Small yield to let the event queue be registered before subscribing.
                tokio::task::yield_now().await;

                let Some(mut bg_reader) = event_queue_mgr.subscribe(&task_id).await else {
                    trace_warn!(
                        task_id = %task_id,
                        "background event processor: no queue to subscribe to"
                    );
                    return;
                };

                // Get the current task from the store.
                let Ok(Some(mut last_task)) = task_store.get(&task_id).await else {
                    return;
                };

                let mut executor_done = false;
                let mut handle_fuse = executor_handle;

                loop {
                    if executor_done {
                        match bg_reader.read().await {
                            Some(event) => {
                                process_event_bg(
                                    event,
                                    &task_id,
                                    &mut last_task,
                                    &*task_store,
                                    &*push_config_store,
                                    push_sender.as_deref(),
                                    &limits,
                                )
                                .await;
                            }
                            None => break,
                        }
                    } else {
                        tokio::select! {
                            biased;
                            event = bg_reader.read() => {
                                match event {
                                    Some(event) => {
                                        process_event_bg(
                                            event,
                                            &task_id,
                                            &mut last_task,
                                            &*task_store,
                                            &*push_config_store,
                                            push_sender.as_deref(),
                                            &limits,
                                        )
                                        .await;
                                    }
                                    None => break,
                                }
                            }
                            result = &mut handle_fuse => {
                                executor_done = true;
                                if result.is_err() {
                                    trace_error!(
                                        task_id = %task_id,
                                        "executor task panicked (background processor)"
                                    );
                                    if !last_task.status.state.is_terminal() {
                                        last_task.status = TaskStatus::with_timestamp(TaskState::Failed);
                                        if let Err(_e) = task_store.save(last_task.clone()).await {
                                            trace_error!(
                                                task_id = %task_id,
                                                "background processor: task store save failed after executor panic"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
        ));
    }
}

// ── Standalone free functions for spawned tasks ─────────────────────────────

/// Standalone event processor for background tasks (avoids borrowing `&self`).
///
/// Used by [`RequestHandler::spawn_background_event_processor`] which runs
/// in a spawned task that can't hold a reference to the handler.
async fn process_event_bg(
    event: a2a_protocol_types::error::A2aResult<StreamResponse>,
    task_id: &TaskId,
    last_task: &mut Task,
    task_store: &dyn TaskStore,
    push_config_store: &dyn PushConfigStore,
    push_sender: Option<&dyn PushSender>,
    limits: &HandlerLimits,
) {
    match event {
        Ok(ref stream_resp @ StreamResponse::StatusUpdate(ref update)) => {
            let current = last_task.status.state;
            let next = update.status.state;
            if !current.can_transition_to(next) {
                trace_warn!(
                    task_id = %task_id,
                    from = %current,
                    to = %next,
                    "invalid state transition rejected (background)"
                );
                return;
            }
            last_task.status = TaskStatus {
                state: next,
                message: update.status.message.clone(),
                timestamp: update.status.timestamp.clone(),
            };
            if let Err(_e) = task_store.save(last_task.clone()).await {
                trace_error!(
                    task_id = %task_id,
                    "background processor: task store save failed for status update"
                );
            }
            deliver_push_bg(task_id, stream_resp, push_config_store, push_sender, limits).await;
        }
        Ok(ref stream_resp @ StreamResponse::ArtifactUpdate(ref update)) => {
            let artifacts = last_task.artifacts.get_or_insert_with(Vec::new);
            artifacts.push(update.artifact.clone());
            if let Err(_e) = task_store.save(last_task.clone()).await {
                trace_error!(
                    task_id = %task_id,
                    "background processor: task store save failed for artifact update"
                );
            }
            deliver_push_bg(task_id, stream_resp, push_config_store, push_sender, limits).await;
        }
        Ok(StreamResponse::Task(task)) => {
            *last_task = task;
            if let Err(_e) = task_store.save(last_task.clone()).await {
                trace_error!(
                    task_id = %task_id,
                    "background processor: task store save failed for task snapshot"
                );
            }
        }
        Ok(StreamResponse::Message(_) | _) => {}
        Err(_e) => {
            last_task.status = TaskStatus::with_timestamp(TaskState::Failed);
            if let Err(_save_err) = task_store.save(last_task.clone()).await {
                trace_error!(
                    task_id = %task_id,
                    "background processor: task store save failed for error state"
                );
            }
        }
    }
}

/// Standalone push delivery for background tasks.
async fn deliver_push_bg(
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use a2a_protocol_types::artifact::{Artifact, ArtifactId};
    use a2a_protocol_types::error::{A2aError, A2aResult};
    use a2a_protocol_types::events::{
        StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent,
    };
    use a2a_protocol_types::message::Part;
    use a2a_protocol_types::push::TaskPushNotificationConfig;
    use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

    use crate::push::{InMemoryPushConfigStore, PushConfigStore};
    use crate::store::InMemoryTaskStore;

    use super::super::super::limits::HandlerLimits;
    use super::*;

    // ── helpers ───────────────────────────────────────────────────────────

    /// A push config store that always returns errors, for testing silent error swallowing.
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

    fn make_task(id: &str, state: TaskState) -> Task {
        Task {
            id: id.into(),
            context_id: ContextId::new("ctx-1"),
            status: TaskStatus::new(state),
            history: None,
            artifacts: None,
            metadata: None,
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

    fn make_artifact_event(task_id: &str) -> StreamResponse {
        StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            task_id: TaskId::new(task_id),
            context_id: ContextId::new("ctx-1"),
            artifact: Artifact::new(ArtifactId::new("art-1"), vec![Part::text("output")]),
            append: None,
            last_chunk: Some(true),
            metadata: None,
        })
    }

    fn default_limits() -> HandlerLimits {
        HandlerLimits::default()
    }

    // ── deliver_push_bg tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn deliver_push_bg_with_no_sender_is_noop() {
        let store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t1");
        let event = make_status_event("t1", TaskState::Working);

        // With push_sender = None the function should return early without error.
        deliver_push_bg(&task_id, &event, &store, None, &default_limits()).await;
        // No panic, no error — test passes.
    }

    #[tokio::test]
    async fn deliver_push_bg_with_failing_store_returns_silently() {
        // The key coverage here is that an Err from the push config store's
        // `list()` is silently swallowed (the function uses `let Ok(configs) = ...`).
        let store = AlwaysErrPushConfigStore;
        let task_id = TaskId::new("t1");
        let event = make_status_event("t1", TaskState::Working);

        // Even though the store errors, deliver_push_bg should not panic or propagate.
        deliver_push_bg(&task_id, &event, &store, None, &default_limits()).await;
    }

    // ── process_event_bg tests ────────────────────────────────────────────

    #[tokio::test]
    async fn process_event_bg_status_update_valid_transition() {
        let task_store = InMemoryTaskStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t1");

        // Seed a task in Submitted state.
        task_store
            .save(make_task("t1", TaskState::Submitted))
            .await
            .unwrap();

        let mut last_task = make_task("t1", TaskState::Submitted);
        let event: A2aResult<StreamResponse> = Ok(make_status_event("t1", TaskState::Working));

        process_event_bg(
            event,
            &task_id,
            &mut last_task,
            &task_store,
            &push_store,
            None,
            &default_limits(),
        )
        .await;

        // last_task should now reflect Working.
        assert_eq!(last_task.status.state, TaskState::Working);

        // Task store should also be updated.
        let stored = task_store.get(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status.state, TaskState::Working);
    }

    #[tokio::test]
    async fn process_event_bg_status_update_invalid_transition_ignored() {
        let task_store = InMemoryTaskStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t1");

        // Start in a terminal state (Completed) — cannot transition to Working.
        task_store
            .save(make_task("t1", TaskState::Completed))
            .await
            .unwrap();
        let mut last_task = make_task("t1", TaskState::Completed);

        let event: A2aResult<StreamResponse> = Ok(make_status_event("t1", TaskState::Working));
        process_event_bg(
            event,
            &task_id,
            &mut last_task,
            &task_store,
            &push_store,
            None,
            &default_limits(),
        )
        .await;

        // State must remain Completed — invalid transition is silently ignored.
        assert_eq!(last_task.status.state, TaskState::Completed);

        let stored = task_store.get(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status.state, TaskState::Completed);
    }

    #[tokio::test]
    async fn process_event_bg_artifact_update_appends() {
        let task_store = InMemoryTaskStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t1");

        task_store
            .save(make_task("t1", TaskState::Working))
            .await
            .unwrap();
        let mut last_task = make_task("t1", TaskState::Working);

        let event: A2aResult<StreamResponse> = Ok(make_artifact_event("t1"));
        process_event_bg(
            event,
            &task_id,
            &mut last_task,
            &task_store,
            &push_store,
            None,
            &default_limits(),
        )
        .await;

        // Artifact should be appended to last_task.
        let artifacts = last_task
            .artifacts
            .as_ref()
            .expect("artifacts should be Some");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].id, ArtifactId::new("art-1"));

        // Store should reflect the artifact too.
        let stored = task_store.get(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.artifacts.as_ref().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn process_event_bg_error_marks_failed() {
        let task_store = InMemoryTaskStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t1");

        task_store
            .save(make_task("t1", TaskState::Working))
            .await
            .unwrap();
        let mut last_task = make_task("t1", TaskState::Working);

        let event: a2a_protocol_types::error::A2aResult<StreamResponse> =
            Err(A2aError::internal("agent failure"));

        process_event_bg(
            event,
            &task_id,
            &mut last_task,
            &task_store,
            &push_store,
            None,
            &default_limits(),
        )
        .await;

        assert_eq!(last_task.status.state, TaskState::Failed);

        let stored = task_store.get(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status.state, TaskState::Failed);
    }

    #[tokio::test]
    async fn process_event_bg_task_snapshot_replaces() {
        let task_store = InMemoryTaskStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t1");

        task_store
            .save(make_task("t1", TaskState::Submitted))
            .await
            .unwrap();
        let mut last_task = make_task("t1", TaskState::Submitted);

        // A Task snapshot event replaces last_task entirely.
        let replacement = make_task("t1", TaskState::Completed);
        let event: A2aResult<StreamResponse> = Ok(StreamResponse::Task(replacement.clone()));

        process_event_bg(
            event,
            &task_id,
            &mut last_task,
            &task_store,
            &push_store,
            None,
            &default_limits(),
        )
        .await;

        assert_eq!(last_task.status.state, TaskState::Completed);

        let stored = task_store.get(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status.state, TaskState::Completed);
    }
}
