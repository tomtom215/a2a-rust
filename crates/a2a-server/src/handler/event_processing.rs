// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Event collection, state-transition processing, and push notification delivery.
//!
//! Contains both the `&self`-based methods (used by sync mode's `collect_events`)
//! and standalone free functions (used by the background event processor in
//! streaming mode, which cannot hold a reference to `RequestHandler`).

use std::sync::Arc;

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

use crate::error::{ServerError, ServerResult};
use crate::push::{PushConfigStore, PushSender};
use crate::store::TaskStore;
use crate::streaming::{EventQueueReader, InMemoryQueueReader};

use super::limits::HandlerLimits;
use super::RequestHandler;

// ── &self methods (sync mode) ───────────────────────────────────────────────

impl RequestHandler {
    /// Collects events until stream closes, updating the task store and
    /// delivering push notifications. Returns the final task.
    ///
    /// Takes the executor's `JoinHandle` so that if the executor panics or
    /// terminates without closing the queue properly, we detect it and avoid
    /// blocking forever (CB-3).
    pub(crate) async fn collect_events(
        &self,
        mut reader: InMemoryQueueReader,
        task_id: TaskId,
        executor_handle: tokio::task::JoinHandle<()>,
    ) -> ServerResult<Task> {
        let mut last_task = self
            .task_store
            .get(&task_id)
            .await?
            .ok_or_else(|| ServerError::TaskNotFound(task_id.clone()))?;

        // Pin the executor handle so we can poll it alongside the reader.
        // When the executor finishes (or panics), we'll drain remaining events
        // and then return, rather than blocking forever.
        let mut executor_done = false;
        let mut handle_fuse = executor_handle;

        loop {
            if executor_done {
                // Executor finished — drain any remaining buffered events.
                match reader.read().await {
                    Some(event) => {
                        self.process_event(event, &task_id, &mut last_task).await?;
                    }
                    None => break,
                }
            } else {
                tokio::select! {
                    biased;
                    event = reader.read() => {
                        match event {
                            Some(event) => {
                                self.process_event(event, &task_id, &mut last_task).await?;
                            }
                            None => break,
                        }
                    }
                    result = &mut handle_fuse => {
                        executor_done = true;
                        if result.is_err() {
                            // Executor panicked (CB-2). Mark task as failed
                            // and drain remaining events.
                            trace_error!(
                                task_id = %task_id,
                                "executor task panicked"
                            );
                            if !last_task.status.state.is_terminal() {
                                last_task.status = TaskStatus::with_timestamp(TaskState::Failed);
                                self.task_store.save(last_task.clone()).await?;
                            }
                        }
                        // Continue to drain remaining events from the queue.
                    }
                }
            }
        }

        Ok(last_task)
    }

    /// Processes a single event from the queue reader, updating the task and
    /// delivering push notifications.
    async fn process_event(
        &self,
        event: a2a_protocol_types::error::A2aResult<StreamResponse>,
        task_id: &TaskId,
        last_task: &mut Task,
    ) -> ServerResult<()> {
        match event {
            Ok(ref stream_resp @ StreamResponse::StatusUpdate(ref update)) => {
                let current = last_task.status.state;
                let next = update.status.state;
                if !current.can_transition_to(next) {
                    trace_warn!(
                        task_id = %task_id,
                        from = %current,
                        to = %next,
                        "invalid state transition rejected"
                    );
                    return Err(ServerError::InvalidStateTransition {
                        task_id: task_id.clone(),
                        from: current,
                        to: next,
                    });
                }
                last_task.status = TaskStatus {
                    state: next,
                    message: update.status.message.clone(),
                    timestamp: update.status.timestamp.clone(),
                };
                self.task_store.save(last_task.clone()).await?;
                self.deliver_push(task_id, stream_resp).await;
            }
            Ok(ref stream_resp @ StreamResponse::ArtifactUpdate(ref update)) => {
                let artifacts = last_task.artifacts.get_or_insert_with(Vec::new);
                artifacts.push(update.artifact.clone());
                self.task_store.save(last_task.clone()).await?;
                self.deliver_push(task_id, stream_resp).await;
            }
            Ok(StreamResponse::Task(task)) => {
                *last_task = task;
                self.task_store.save(last_task.clone()).await?;
            }
            Ok(StreamResponse::Message(_) | _) => {
                // Messages and future stream response variants — continue.
            }
            Err(e) => {
                last_task.status = TaskStatus::with_timestamp(TaskState::Failed);
                self.task_store.save(last_task.clone()).await?;
                return Err(ServerError::Protocol(e));
            }
        }
        Ok(())
    }

    /// Delivers push notifications for a streaming event if configs exist.
    ///
    /// Push deliveries are sequential per-config, but each delivery is bounded
    /// by a timeout to prevent one slow webhook from blocking all subsequent
    /// deliveries indefinitely.
    async fn deliver_push(&self, task_id: &TaskId, event: &StreamResponse) {
        let Some(ref sender) = self.push_sender else {
            return;
        };
        let Ok(configs) = self.push_config_store.list(task_id.as_ref()).await else {
            return;
        };
        for config in &configs {
            let result = tokio::time::timeout(
                self.limits.push_delivery_timeout,
                sender.send(&config.url, event, config),
            )
            .await;
            match result {
                Ok(Err(_err)) => {
                    trace_warn!(
                        task_id = %task_id,
                        url = %config.url,
                        error = %_err,
                        "push notification delivery failed"
                    );
                }
                Err(_) => {
                    trace_warn!(
                        task_id = %task_id,
                        url = %config.url,
                        "push notification delivery timed out"
                    );
                }
                Ok(Ok(())) => {}
            }
        }
    }
}

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
                                        let _ = task_store.save(last_task.clone()).await;
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
            let _ = task_store.save(last_task.clone()).await;
            deliver_push_bg(task_id, stream_resp, push_config_store, push_sender, limits).await;
        }
        Ok(ref stream_resp @ StreamResponse::ArtifactUpdate(ref update)) => {
            let artifacts = last_task.artifacts.get_or_insert_with(Vec::new);
            artifacts.push(update.artifact.clone());
            let _ = task_store.save(last_task.clone()).await;
            deliver_push_bg(task_id, stream_resp, push_config_store, push_sender, limits).await;
        }
        Ok(StreamResponse::Task(task)) => {
            *last_task = task;
            let _ = task_store.save(last_task.clone()).await;
        }
        Ok(StreamResponse::Message(_) | _) => {}
        Err(_e) => {
            last_task.status = TaskStatus::with_timestamp(TaskState::Failed);
            let _ = task_store.save(last_task.clone()).await;
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
