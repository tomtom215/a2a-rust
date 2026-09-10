// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The two agents under test: one that streams in steps and can be made to
//! "die" mid-task, one that fails its first N attempts.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::claim_one;
use std::time::Duration;

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::executor_helpers::EventEmitter;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::TaskState;

/// The artifact every executor here writes to.
pub const ARTIFACT_ID: &str = "report";
/// Text of the first chunk.
pub const CHUNK_ONE: &str = "chunk one; ";
/// Text of the second chunk.
pub const CHUNK_TWO: &str = "chunk two.";

/// Streams `Working`, an artifact in two chunks, then `Completed`, pausing
/// `step` between frames.
///
/// With `die_after_first_chunk` set, the executor emits the first chunk and
/// then parks forever: nothing after that point is ever written. That is what
/// a process crash looks like from the store's side — the rows written so far
/// are there, the rest never arrive — and it is what Act 1 uses to show which
/// of them a fresh handler can read back.
pub struct StreamingExecutor {
    /// Delay between frames, so a subscriber elsewhere has a window in which
    /// the task exists and is non-terminal.
    pub step: Duration,
    /// Park forever after the first chunk instead of finishing.
    pub die_after_first_chunk: bool,
}

impl AgentExecutor for StreamingExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            tokio::time::sleep(self.step).await;
            emit.artifact(ARTIFACT_ID, vec![Part::text(CHUNK_ONE)], None, Some(false))
                .await?;
            if self.die_after_first_chunk {
                // The process died here. Nothing below runs, and nothing
                // resumes it — see Act 1's second check for what that means.
                std::future::pending::<()>().await;
            }
            tokio::time::sleep(self.step).await;
            emit.artifact(
                ARTIFACT_ID,
                vec![Part::text(CHUNK_TWO)],
                Some(true),
                Some(true),
            )
            .await?;
            emit.status(TaskState::Completed).await
        })
    }
}

/// What a [`FlakyExecutor`] has done so far, shared with whoever built it.
///
/// The handler owns its executor by value, so the counts live behind an
/// `Arc` the act keeps a clone of. The question Act 2 asks is not "does the
/// task end up failed" but "how many times did the SDK call me" — a retrying
/// handler would show more invocations than sends.
#[derive(Debug)]
pub struct FlakyTally {
    failures_left: AtomicU32,
    invocations: AtomicU32,
}

impl FlakyTally {
    /// A tally that will inject `failures` failures before succeeding.
    pub const fn failing_first(failures: u32) -> Self {
        Self {
            failures_left: AtomicU32::new(failures),
            invocations: AtomicU32::new(0),
        }
    }

    /// How many times `execute` has been called.
    pub fn invocations(&self) -> u32 {
        self.invocations.load(Ordering::SeqCst)
    }
}

/// Fails the first N invocations with an injected error, then completes
/// normally. N and the invocation count live in the shared [`FlakyTally`].
pub struct FlakyExecutor(pub Arc<FlakyTally>);

impl AgentExecutor for FlakyExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let tally = &self.0;
            let attempt = tally.invocations.fetch_add(1, Ordering::SeqCst) + 1;
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            // Decrement-if-positive, so concurrent invocations cannot drive
            // the counter below zero.
            let inject = claim_one(&tally.failures_left);
            if inject {
                return Err(A2aError::internal(format!(
                    "injected failure on attempt {attempt}"
                )));
            }
            emit.artifact(
                ARTIFACT_ID,
                vec![Part::text(format!("succeeded on attempt {attempt}"))],
                None,
                Some(true),
            )
            .await?;
            emit.status(TaskState::Completed).await
        })
    }
}
