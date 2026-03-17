// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Background event processing for streaming mode.
//!
//! # Module structure
//!
//! | Module | Responsibility |
//! |---|---|
//! | (this file) | Event loop orchestration and executor lifecycle |
//! | [`state_machine`] | Event dispatch, state transitions, task store updates |
//! | [`push_delivery`] | Push notification delivery to webhook endpoints |

mod push_delivery;
mod state_machine;

use std::sync::Arc;

use a2a_protocol_types::task::{TaskId, TaskState, TaskStatus};

use crate::streaming::EventQueueReader;

use super::super::RequestHandler;

use state_machine::process_event_bg;

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
