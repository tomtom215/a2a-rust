// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Event processing state machine for background tasks.
//!
//! Handles state transitions, task store updates, and artifact accumulation
//! for streaming events received by the background event processor.

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

use crate::handler::limits::HandlerLimits;
use crate::push::{PushConfigStore, PushSender};
use crate::store::TaskStore;

use super::push_delivery::deliver_push_bg;

/// Processes a single streaming event: validates state transitions, updates the
/// task store, and triggers push delivery.
///
/// Used by [`super::spawn_background_event_processor`] which runs in a spawned
/// task that can't hold a reference to the handler.
/// Returns the number of consecutive store failures encountered.
///
/// When a save fails, the in-memory `last_task` is reverted to its previous
/// state so it stays consistent with what's actually persisted. This prevents
/// a cascade of phantom state that was never written to the store.
#[allow(clippy::too_many_lines)]
pub(super) async fn process_event_bg(
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
                // FIX(#6): Match sync-mode behavior — invalid transitions are errors,
                // not silent warnings. Mark the task as failed so the state is
                // consistent regardless of transport mode.
                trace_error!(
                    task_id = %task_id,
                    from = %current,
                    to = %next,
                    "invalid state transition rejected (background); marking task as failed"
                );
                last_task.status = TaskStatus::with_timestamp(TaskState::Failed);
                if let Err(_e) = task_store.save(last_task.clone()).await {
                    trace_error!(
                        task_id = %task_id,
                        error = %_e,
                        "background processor: failed to persist failed state after invalid transition"
                    );
                }
                return;
            }
            // Save previous state so we can revert on failure.
            let prev_status = last_task.status.clone();
            last_task.status = TaskStatus {
                state: next,
                message: update.status.message.clone(),
                timestamp: update.status.timestamp.clone(),
            };
            if let Err(_e) = task_store.save(last_task.clone()).await {
                trace_error!(
                    task_id = %task_id,
                    error = %_e,
                    "background processor: task store save failed for status update; reverting in-memory state"
                );
                // Revert in-memory state to stay consistent with the store.
                last_task.status = prev_status;
                return;
            }
            deliver_push_bg(task_id, stream_resp, push_config_store, push_sender, limits).await;
        }
        Ok(ref stream_resp @ StreamResponse::ArtifactUpdate(ref update)) => {
            let artifacts = last_task.artifacts.get_or_insert_with(Vec::new);
            if artifacts.len() >= limits.max_artifacts_per_task {
                trace_warn!(
                    task_id = %task_id,
                    max = limits.max_artifacts_per_task,
                    "artifact limit reached; dropping artifact update"
                );
                return;
            }
            artifacts.push(update.artifact.clone());
            if let Err(_e) = task_store.save(last_task.clone()).await {
                trace_error!(
                    task_id = %task_id,
                    error = %_e,
                    "background processor: task store save failed for artifact update; reverting"
                );
                // Revert: remove the artifact we just pushed.
                if let Some(ref mut arts) = last_task.artifacts {
                    arts.pop();
                }
                return;
            }
            deliver_push_bg(task_id, stream_resp, push_config_store, push_sender, limits).await;
        }
        Ok(StreamResponse::Task(task)) => {
            let prev = last_task.clone();
            *last_task = task;
            if let Err(_e) = task_store.save(last_task.clone()).await {
                trace_error!(
                    task_id = %task_id,
                    error = %_e,
                    "background processor: task store save failed for task snapshot; reverting"
                );
                *last_task = prev;
            }
        }
        Ok(StreamResponse::Message(_) | _) => {}
        Err(_e) => {
            let prev_status = last_task.status.clone();
            last_task.status = TaskStatus::with_timestamp(TaskState::Failed);
            if let Err(_save_err) = task_store.save(last_task.clone()).await {
                trace_error!(
                    task_id = %task_id,
                    original_error = %_e,
                    save_error = %_save_err,
                    "background processor: task store save failed for error state; reverting"
                );
                last_task.status = prev_status;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::artifact::{Artifact, ArtifactId};
    use a2a_protocol_types::error::{A2aError, A2aResult};
    use a2a_protocol_types::events::{
        StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent,
    };
    use a2a_protocol_types::message::Part;
    use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

    use crate::handler::limits::HandlerLimits;
    use crate::push::InMemoryPushConfigStore;
    use crate::store::InMemoryTaskStore;

    use super::*;

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

    #[tokio::test]
    async fn process_event_bg_status_update_valid_transition() {
        let task_store = InMemoryTaskStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t1");

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

        assert_eq!(last_task.status.state, TaskState::Working);
        let stored = task_store.get(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status.state, TaskState::Working);
    }

    #[tokio::test]
    async fn process_event_bg_status_update_invalid_transition_marks_failed() {
        // FIX(#6): Invalid transitions now mark the task as Failed for
        // consistency with sync mode behavior.
        let task_store = InMemoryTaskStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t1");

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

        assert_eq!(last_task.status.state, TaskState::Failed);
        let stored = task_store.get(&task_id).await.unwrap().unwrap();
        assert_eq!(stored.status.state, TaskState::Failed);
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

        let artifacts = last_task
            .artifacts
            .as_ref()
            .expect("artifacts should be Some");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].id, ArtifactId::new("art-1"));

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

    // ── Failing task store for revert-path coverage ──────────────────────

    use std::future::Future;
    use std::pin::Pin;

    /// A task store that succeeds on get but always fails on save.
    struct FailingSaveStore {
        inner: InMemoryTaskStore,
    }

    impl FailingSaveStore {
        fn new() -> Self {
            Self {
                inner: InMemoryTaskStore::new(),
            }
        }
    }

    impl crate::store::TaskStore for FailingSaveStore {
        fn save<'a>(
            &'a self,
            _task: Task,
        ) -> Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>>
        {
            Box::pin(async { Err(A2aError::internal("simulated save failure")) })
        }
        fn get<'a>(
            &'a self,
            id: &'a TaskId,
        ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
            self.inner.get(id)
        }
        fn list<'a>(
            &'a self,
            p: &'a a2a_protocol_types::params::ListTasksParams,
        ) -> Pin<
            Box<
                dyn Future<Output = A2aResult<a2a_protocol_types::responses::TaskListResponse>>
                    + Send
                    + 'a,
            >,
        > {
            self.inner.list(p)
        }
        fn insert_if_absent<'a>(
            &'a self,
            task: Task,
        ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
            self.inner.insert_if_absent(task)
        }
        fn delete<'a>(
            &'a self,
            id: &'a TaskId,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            self.inner.delete(id)
        }
    }

    #[tokio::test]
    async fn status_update_save_failure_reverts_in_memory_state() {
        let task_store = FailingSaveStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t-revert");

        // Seed the inner store via insert_if_absent (which delegates to inner).
        task_store
            .inner
            .save(make_task("t-revert", TaskState::Submitted))
            .await
            .unwrap();
        let mut last_task = make_task("t-revert", TaskState::Submitted);

        let event: A2aResult<StreamResponse> =
            Ok(make_status_event("t-revert", TaskState::Working));
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

        // In-memory state should be reverted to Submitted since save failed.
        assert_eq!(
            last_task.status.state,
            TaskState::Submitted,
            "in-memory state should revert on save failure"
        );
    }

    #[tokio::test]
    async fn artifact_update_save_failure_reverts_artifact_list() {
        let task_store = FailingSaveStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t-art-revert");

        task_store
            .inner
            .save(make_task("t-art-revert", TaskState::Working))
            .await
            .unwrap();
        let mut last_task = make_task("t-art-revert", TaskState::Working);

        let event: A2aResult<StreamResponse> = Ok(make_artifact_event("t-art-revert"));
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

        // Artifact should be popped since save failed.
        assert!(
            last_task.artifacts.as_ref().is_none_or(Vec::is_empty),
            "artifact should be reverted on save failure"
        );
    }

    #[tokio::test]
    async fn task_snapshot_save_failure_reverts_to_previous() {
        let task_store = FailingSaveStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t-snap-revert");

        task_store
            .inner
            .save(make_task("t-snap-revert", TaskState::Submitted))
            .await
            .unwrap();
        let mut last_task = make_task("t-snap-revert", TaskState::Submitted);

        let replacement = make_task("t-snap-revert", TaskState::Completed);
        let event: A2aResult<StreamResponse> = Ok(StreamResponse::Task(replacement));
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

        assert_eq!(
            last_task.status.state,
            TaskState::Submitted,
            "task snapshot should revert on save failure"
        );
    }

    #[tokio::test]
    async fn error_event_save_failure_reverts_status() {
        let task_store = FailingSaveStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t-err-revert");

        task_store
            .inner
            .save(make_task("t-err-revert", TaskState::Working))
            .await
            .unwrap();
        let mut last_task = make_task("t-err-revert", TaskState::Working);

        let event: A2aResult<StreamResponse> = Err(A2aError::internal("agent failure"));
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

        assert_eq!(
            last_task.status.state,
            TaskState::Working,
            "error state should revert on save failure"
        );
    }

    #[tokio::test]
    async fn artifact_limit_enforced() {
        let task_store = InMemoryTaskStore::new();
        let push_store = InMemoryPushConfigStore::new();
        let task_id = TaskId::new("t-limit");

        task_store
            .save(make_task("t-limit", TaskState::Working))
            .await
            .unwrap();
        let mut last_task = make_task("t-limit", TaskState::Working);

        // Set limit to 1 artifact.
        let limits = HandlerLimits::default().with_max_artifacts_per_task(1);

        // First artifact should succeed.
        let event: A2aResult<StreamResponse> = Ok(make_artifact_event("t-limit"));
        process_event_bg(
            event,
            &task_id,
            &mut last_task,
            &task_store,
            &push_store,
            None,
            &limits,
        )
        .await;
        assert_eq!(last_task.artifacts.as_ref().unwrap().len(), 1);

        // Second should be dropped.
        let event: A2aResult<StreamResponse> = Ok(make_artifact_event("t-limit"));
        process_event_bg(
            event,
            &task_id,
            &mut last_task,
            &task_store,
            &push_store,
            None,
            &limits,
        )
        .await;
        assert_eq!(
            last_task.artifacts.as_ref().unwrap().len(),
            1,
            "artifact count should not exceed limit"
        );
    }
}
