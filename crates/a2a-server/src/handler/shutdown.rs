// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Graceful shutdown methods for [`super::RequestHandler`].

use std::time::{Duration, Instant};

use super::RequestHandler;

impl RequestHandler {
    /// Initiates graceful shutdown of the handler.
    ///
    /// This method:
    /// 1. Cancels all in-flight tasks by signalling their cancellation tokens.
    /// 2. Destroys all event queues, causing readers to see EOF.
    ///
    /// After calling `shutdown()`, new requests will still be accepted but
    /// in-flight tasks will observe cancellation. The caller should stop
    /// accepting new connections after calling this method.
    pub async fn shutdown(&self) {
        // Cancel all in-flight tasks.
        {
            let tokens = self.cancellation_tokens.read().await;
            for entry in tokens.values() {
                entry.token.cancel();
            }
        }

        // Destroy all event queues so readers see EOF.
        self.event_queue_manager.destroy_all().await;

        // Clear cancellation tokens.
        {
            let mut tokens = self.cancellation_tokens.write().await;
            tokens.clear();
        }

        // Give executor a chance to clean up resources (bounded to avoid hanging).
        let _ = tokio::time::timeout(Duration::from_secs(10), self.executor.on_shutdown()).await;
    }

    /// Initiates graceful shutdown with a timeout.
    ///
    /// Cancels all in-flight tasks and waits up to `timeout` for event queues
    /// to drain before force-destroying them. This gives executors a chance
    /// to finish writing final events before the queues are torn down.
    pub async fn shutdown_with_timeout(&self, timeout: Duration) {
        // Cancel all in-flight tasks.
        {
            let tokens = self.cancellation_tokens.read().await;
            for entry in tokens.values() {
                entry.token.cancel();
            }
        }

        // Wait for event queues to drain (executors to finish), with timeout.
        let drain_start = Instant::now();
        loop {
            let active = self.event_queue_manager.active_count().await;
            if active == 0 {
                break;
            }
            if drain_start.elapsed() >= timeout {
                trace_warn!(
                    active_queues = active,
                    "shutdown timeout reached, force-destroying remaining queues"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Destroy all remaining event queues.
        self.event_queue_manager.destroy_all().await;

        // Clear cancellation tokens.
        {
            let mut tokens = self.cancellation_tokens.write().await;
            tokens.clear();
        }

        // Give executor a chance to clean up resources (bounded by the same timeout
        // to avoid hanging if the executor blocks during cleanup).
        let _ = tokio::time::timeout(timeout, self.executor.on_shutdown()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use a2a_protocol_types::error::A2aResult;
    use std::future::Future;
    use std::pin::Pin;

    use crate::builder::RequestHandlerBuilder;
    use crate::executor::AgentExecutor;
    use crate::request_context::RequestContext;
    use crate::streaming::EventQueueWriter;

    /// Minimal no-op executor for shutdown tests.
    struct NoopExecutor;

    impl AgentExecutor for NoopExecutor {
        fn execute<'a>(
            &'a self,
            _ctx: &'a RequestContext,
            _queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
    }

    /// Builds a minimal `RequestHandler` suitable for shutdown tests.
    fn make_handler() -> RequestHandler {
        RequestHandlerBuilder::new(NoopExecutor)
            .build()
            .expect("builder should succeed with defaults")
    }

    // ── shutdown ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn shutdown_completes_without_panic() {
        let handler = make_handler();
        // shutdown on a fresh handler with no in-flight tasks should complete cleanly.
        handler.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let handler = make_handler();
        handler.shutdown().await;
        // Calling shutdown a second time should not panic or deadlock.
        handler.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_clears_cancellation_tokens() {
        let handler = make_handler();

        // Insert a synthetic cancellation entry.
        {
            let mut tokens = handler.cancellation_tokens.write().await;
            tokens.insert(
                a2a_protocol_types::task::TaskId::new("t-1"),
                super::super::CancellationEntry {
                    token: tokio_util::sync::CancellationToken::new(),
                    created_at: Instant::now(),
                },
            );
        }
        assert_eq!(
            handler.cancellation_tokens.read().await.len(),
            1,
            "should have 1 token before shutdown"
        );

        handler.shutdown().await;

        assert!(
            handler.cancellation_tokens.read().await.is_empty(),
            "cancellation tokens should be cleared after shutdown"
        );
    }

    // ── shutdown_with_timeout ──────────────────────────────────────────────

    #[tokio::test]
    async fn shutdown_with_timeout_completes_within_timeout() {
        let handler = make_handler();
        let start = Instant::now();
        handler.shutdown_with_timeout(Duration::from_secs(5)).await;
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "shutdown with no active queues should complete well before the timeout"
        );
    }

    #[tokio::test]
    async fn shutdown_with_timeout_clears_cancellation_tokens() {
        let handler = make_handler();

        {
            let mut tokens = handler.cancellation_tokens.write().await;
            tokens.insert(
                a2a_protocol_types::task::TaskId::new("t-2"),
                super::super::CancellationEntry {
                    token: tokio_util::sync::CancellationToken::new(),
                    created_at: Instant::now(),
                },
            );
        }

        handler
            .shutdown_with_timeout(Duration::from_millis(200))
            .await;

        assert!(
            handler.cancellation_tokens.read().await.is_empty(),
            "cancellation tokens should be cleared after shutdown_with_timeout"
        );
    }

    #[tokio::test]
    async fn shutdown_with_timeout_cancels_tokens() {
        let handler = make_handler();
        let token = tokio_util::sync::CancellationToken::new();
        let token_clone = token.clone();

        {
            let mut tokens = handler.cancellation_tokens.write().await;
            tokens.insert(
                a2a_protocol_types::task::TaskId::new("t-3"),
                super::super::CancellationEntry {
                    token: token_clone,
                    created_at: Instant::now(),
                },
            );
        }

        handler
            .shutdown_with_timeout(Duration::from_millis(200))
            .await;

        assert!(
            token.is_cancelled(),
            "cancellation token should be cancelled after shutdown"
        );
    }

    #[tokio::test]
    async fn shutdown_with_zero_timeout_still_completes() {
        let handler = make_handler();
        // A zero-duration timeout should not panic or hang.
        handler
            .shutdown_with_timeout(Duration::from_millis(0))
            .await;
    }

    #[tokio::test]
    async fn shutdown_with_timeout_drains_active_queues() {
        // Covers lines 62-64, 68-70: the drain loop that waits for active
        // queues to reach zero before the timeout expires.
        use a2a_protocol_types::task::TaskId;

        let handler = make_handler();
        let task_id = TaskId::new("t-drain");

        // Create an active event queue so active_count() > 0.
        let (_writer, _reader) = handler.event_queue_manager.get_or_create(&task_id).await;
        assert_eq!(
            handler.event_queue_manager.active_count().await,
            1,
            "should have 1 active queue before shutdown"
        );

        // Spawn a task that destroys the queue after a short delay, simulating
        // an executor finishing before the timeout.
        let eqm = handler.event_queue_manager.clone();
        let tid = task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            eqm.destroy(&tid).await;
        });

        let start = Instant::now();
        handler.shutdown_with_timeout(Duration::from_secs(5)).await;
        // The drain loop should have detected the queue was removed and exited
        // well before the 5-second timeout.
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "shutdown should complete quickly once queues drain"
        );
    }

    #[tokio::test]
    async fn shutdown_with_timeout_force_destroys_on_timeout() {
        // Covers lines 105-111: the timeout path where active queues remain
        // when the timeout expires, triggering force-destroy.
        use a2a_protocol_types::task::TaskId;

        let handler = make_handler();
        let task_id = TaskId::new("t-force");

        // Create an active event queue that will NOT be drained.
        let (_writer, _reader) = handler.event_queue_manager.get_or_create(&task_id).await;
        assert_eq!(
            handler.event_queue_manager.active_count().await,
            1,
            "should have 1 active queue before shutdown"
        );

        // Use a very short timeout so the drain loop times out.
        let start = Instant::now();
        handler
            .shutdown_with_timeout(Duration::from_millis(100))
            .await;

        // Should complete around the timeout duration.
        assert!(
            start.elapsed() >= Duration::from_millis(100),
            "shutdown should wait at least the timeout duration"
        );
        // After shutdown, queues should be force-destroyed.
        assert_eq!(
            handler.event_queue_manager.active_count().await,
            0,
            "all queues should be destroyed after shutdown timeout"
        );
    }
}
