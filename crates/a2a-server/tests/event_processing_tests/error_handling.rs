// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Executor error, push timeout, and empty executor tests.
//!
//! These tests cover edge cases: executors that emit some events before
//! failing, push senders that hang indefinitely (testing timeout behaviour),
//! the streaming-mode store update after an executor error, and executors
//! that emit no events at all.

use super::*;

// ── Work-then-error executor ────────────────────────────────────────────────

/// Executor that emits Working, then returns an error.
struct WorkThenErrorExecutor;

impl AgentExecutor for WorkThenErrorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            Err(A2aError::internal("executor failed mid-flight"))
        })
    }
}

// ── Sync mode error tests ───────────────────────────────────────────────────

#[tokio::test]
async fn sync_mode_work_then_error_marks_failed() {
    let handler = RequestHandlerBuilder::new(WorkThenErrorExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await;

    match result {
        Ok(send_result) => {
            let task = extract_task(send_result);
            assert_eq!(
                task.status.state,
                TaskState::Failed,
                "executor error after Working should leave task in Failed state"
            );
        }
        Err(e) => {
            // Error propagation is also acceptable — verify the error message.
            let err_msg = format!("{e:?}");
            assert!(
                err_msg.contains("executor failed mid-flight"),
                "expected error to contain 'executor failed mid-flight', got: {err_msg}"
            );
        }
    }
}

// ── Streaming mode error tests ──────────────────────────────────────────────

#[tokio::test]
async fn streaming_mode_work_then_error_marks_failed_in_store() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    struct WaitingWorkThenErrorExecutor {
        proceed: Arc<Notify>,
        started: Arc<AtomicBool>,
    }

    impl AgentExecutor for WaitingWorkThenErrorExecutor {
        fn execute<'a>(
            &'a self,
            ctx: &'a RequestContext,
            queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async move {
                self.started.store(true, Ordering::SeqCst);
                self.proceed.notified().await;
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(TaskState::Working),
                        metadata: None,
                    }))
                    .await?;
                Err(A2aError::internal("deliberate error after working"))
            })
        }
    }

    let proceed = Arc::new(Notify::new());
    let started = Arc::new(AtomicBool::new(false));

    let handler = Arc::new(
        RequestHandlerBuilder::new(WaitingWorkThenErrorExecutor {
            proceed: proceed.clone(),
            started: started.clone(),
        })
        .build()
        .expect("build handler"),
    );

    let handler_clone = handler.clone();
    let send_handle = tokio::spawn(async move {
        let result = handler_clone
            .on_send_message(make_send_params(), true, None)
            .await
            .expect("send message");
        match result {
            SendMessageResult::Stream(mut reader) => {
                let mut saw_working = false;
                let mut saw_failed = false;
                while let Some(event) = reader.read().await {
                    match event {
                        Ok(StreamResponse::StatusUpdate(u))
                            if u.status.state == TaskState::Working =>
                        {
                            saw_working = true;
                        }
                        Ok(StreamResponse::StatusUpdate(u))
                            if u.status.state == TaskState::Failed =>
                        {
                            saw_failed = true;
                        }
                        _ => {}
                    }
                }
                assert!(saw_working, "should have seen Working status");
                assert!(saw_failed, "should have seen Failed status");
            }
            _ => panic!("expected Stream"),
        }
    });

    while !started.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    proceed.notify_one();
    send_handle.await.expect("send handle");

    // Poll until background processor updates the store.
    let mut found = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let list = handler
            .on_list_tasks(default_list_params(), None)
            .await
            .expect("list tasks");
        if list
            .tasks
            .iter()
            .any(|t| t.status.state == TaskState::Failed)
        {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "background processor should mark task as Failed after executor error"
    );
}

#[tokio::test]
async fn streaming_mode_error_marks_failed_in_store() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    struct WaitingErrorExecutor {
        proceed: Arc<Notify>,
        started: Arc<AtomicBool>,
    }

    impl AgentExecutor for WaitingErrorExecutor {
        fn execute<'a>(
            &'a self,
            _ctx: &'a RequestContext,
            _queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async move {
                self.started.store(true, Ordering::SeqCst);
                self.proceed.notified().await;
                Err(A2aError::internal("deliberate error"))
            })
        }
    }

    let proceed = Arc::new(Notify::new());
    let started = Arc::new(AtomicBool::new(false));

    let handler = Arc::new(
        RequestHandlerBuilder::new(WaitingErrorExecutor {
            proceed: proceed.clone(),
            started: started.clone(),
        })
        .build()
        .expect("build handler"),
    );

    let handler_clone = handler.clone();
    let send_handle = tokio::spawn(async move {
        let result = handler_clone
            .on_send_message(make_send_params(), true, None)
            .await
            .expect("send message");
        match result {
            SendMessageResult::Stream(mut reader) => {
                while let Some(_event) = reader.read().await {}
            }
            _ => panic!("expected Stream"),
        }
    });

    while !started.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    proceed.notify_one();
    send_handle.await.expect("send handle");

    // Poll until background processor updates the store.
    let mut found = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let list = handler
            .on_list_tasks(default_list_params(), None)
            .await
            .expect("list tasks");
        if list
            .tasks
            .iter()
            .any(|t| t.status.state == TaskState::Failed)
        {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "background processor should mark task as Failed after error event"
    );
}

// ── Push delivery timeout test ──────────────────────────────────────────────

#[tokio::test]
async fn sync_mode_push_delivery_timeout_does_not_block_forever() {
    use a2a_protocol_server::handler::HandlerLimits;

    let limits =
        HandlerLimits::default().with_push_delivery_timeout(std::time::Duration::from_millis(50));

    let handler = RequestHandlerBuilder::new(StatusExecutor)
        .with_push_sender(SleepForeverPushSender)
        .with_handler_limits(limits)
        .build()
        .expect("build handler");

    // The push sender sleeps forever, but the delivery timeout is 50ms.
    let start = std::time::Instant::now();
    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await
        .expect("send should not hang");
    let elapsed = start.elapsed();

    let task = extract_task(result);
    assert_eq!(task.status.state, TaskState::Completed);
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "handler should complete well within 2 seconds; elapsed: {elapsed:?}"
    );
}

// ── Empty executor test ─────────────────────────────────────────────────────

#[tokio::test]
async fn sync_mode_empty_executor_returns_submitted() {
    let handler = RequestHandlerBuilder::new(EmptyExecutor)
        .build()
        .expect("build handler");

    let task = extract_task(
        handler
            .on_send_message(make_send_params(), false, None)
            .await
            .expect("send"),
    );
    assert_eq!(
        task.status.state,
        TaskState::Submitted,
        "no events emitted; task should stay Submitted"
    );
}
