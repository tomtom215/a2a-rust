// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`TerminalConfirmation`]: what a stream is told of a terminal event the store
//! did not confirm.

/// What a streaming client is told when the task store does not confirm a
/// terminal event the executor emitted.
///
/// A terminal event is unconfirmed when its write failed and a read back shows
/// the task is not in that state, when the store did not answer within the
/// queue's write timeout, or when the processor that writes it had stopped.
/// See [`HandlerLimits::terminal_confirmation`](super::HandlerLimits::terminal_confirmation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TerminalConfirmation {
    /// The stream ends with an `UNAVAILABLE` error: the completion could not
    /// be confirmed, and `GetTask` says what the store holds. A client is
    /// never told a task finished that the store does not record as finished.
    /// The default.
    #[default]
    Required,
    /// The terminal event is delivered as the executor emitted it whether or
    /// not the store confirmed it, as every release before 0.16.0 did. The
    /// stream and `GetTask` can then disagree. For a deployment that prefers
    /// an answer to a truthful one.
    BestEffort,
}
