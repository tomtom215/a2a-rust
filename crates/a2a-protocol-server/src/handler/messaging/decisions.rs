// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Pure decision helpers for the message send path.
//!
//! Free functions with no `self` and no I/O, split out of `mod.rs` so the send
//! path reads as a sequence of decisions rather than a sequence of decisions
//! interleaved with the predicates behind them. Each one is separately
//! testable, which matters because their edge cases (an aged token, a
//! still-in-use context lock) are exactly what a mutation test probes.

use std::time::Instant;

use super::super::helpers::truncate_history;
use super::super::CancellationEntry;
use a2a_protocol_types::task::Task;

/// Hard cap on the number of messages retained in `Task.history`.
///
/// Oldest messages are dropped first. Bounds per-task memory for
/// long-running multi-turn conversations; `GetTask`'s `historyLength`
/// further truncates what is returned to clients.
pub const MAX_TASK_HISTORY_MESSAGES: usize = 1024;

/// Shapes the history carried by a *send response* (or streaming snapshot)
/// per `SendMessageConfiguration.historyLength`.
///
/// The store always keeps the full (capped) history — this only governs the
/// response payload. The default (`None`) omits history entirely: the
/// sender already holds the message it just sent, and echoing it back
/// doubled response payloads for large sends (the 1 MiB benchmark tripped
/// the regression gate at +95% median). `Some(0)` also omits; `Some(n)`
/// keeps the `n` most recent messages, mirroring `GetTask` semantics.
pub(super) fn shape_response_history(task: &mut Task, history_length: Option<u32>) {
    let history = task.history.take();
    // `None` omits history entirely, which is why this is `and_then` over the
    // requested length rather than a call with a default: absent and `Some(0)`
    // both yield `None` here, but only the latter reaches `truncate_history`.
    task.history = history_length.and_then(|n| truncate_history(history, n));
}

/// Returns the JSON-serialized byte length of a value without allocating a `String`.
pub(super) fn json_byte_len(value: &serde_json::Value) -> serde_json::Result<usize> {
    struct CountWriter(usize);
    impl std::io::Write for CountWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut w = CountWriter(0);
    serde_json::to_writer(&mut w, value)?;
    Ok(w.0)
}

// ── Send-path decision helpers ────────────────────────────────────────────────
//
// Extracted from `send_message_inner` so the branch conditions are unit-testable
// in isolation (the enclosing async handler is not easily driven to these exact
// states).

/// A second `SendMessage` targeting a task that still has a **live**
/// (non-cancelled) cancellation token must be rejected: an executor is already
/// in flight for that `task_id`.
pub(super) fn second_send_blocked(entry: &CancellationEntry) -> bool {
    !entry.token.is_cancelled()
}

/// Whether a non-cancelled cancellation token has aged at or past
/// `max_token_age` and is therefore a candidate for the stale-token sweep.
pub(super) fn token_aged(elapsed: std::time::Duration, max_token_age: std::time::Duration) -> bool {
    elapsed >= max_token_age
}

/// Whether an aged token should actually be evicted: only when its event queue
/// is gone (the executor has finished). A token whose queue is still live is
/// kept so the running task stays cancelable.
pub(super) const fn evict_aged_token(queue_live: bool) -> bool {
    !queue_live
}

/// Re-validates, under the write lock, that a sweep candidate is still
/// evictable at removal time.
///
/// Between the read-lock candidate collection and the write-lock removal, a
/// concurrent send can replace the entry with a **fresh, live** token for the
/// same task id (a cancel-then-resend race: the cancelled token passes the
/// in-flight check, and the resend inserts its own token). Removing by id
/// unconditionally would delete that live token and leave the resent executor
/// uncancelable for its whole run — so only entries that are *still* cancelled
/// or *still* aged are removed. A freshly-inserted token is neither.
pub(super) fn token_still_evictable(
    entry: &CancellationEntry,
    now: Instant,
    max_token_age: std::time::Duration,
) -> bool {
    entry.token.is_cancelled() || token_aged(now.duration_since(entry.created_at), max_token_age)
}
