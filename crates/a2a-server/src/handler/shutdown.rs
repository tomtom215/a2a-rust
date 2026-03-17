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
