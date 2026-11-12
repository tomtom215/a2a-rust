// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What to do with a push reservation whose reserver is gone: arm it or remove
//! it, from what the task store holds (ADR 0037, audit H14). Pure functions of
//! what was read; the reading is [`settling`](super::settling)'s.

use a2a_protocol_types::events::StreamResponse;

/// What to do with a reservation whose reserver is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The event is stored: deliver it.
    Arm,
    /// It is not: it never was, and never will be, so no webhook is told.
    Release,
}

/// Whether the event reserved at `log_seq` was stored, from what its task's
/// store holds.
///
/// `position` is the log position the stored task reflects (`None`: the task
/// is gone, or the store records none); `logged` is the event the log holds
/// at `log_seq`, if it holds one.
///
/// * The stored task has not reached `log_seq`: the fold never saved the
///   event. A non-terminal event is logged before it is folded, so the log
///   may hold it all the same; the task did not take it.
/// * It has, and the log holds a *different* event at that position: the
///   position was taken by another turn after the reserver stopped (the
///   numbering of a turn starts past the log's end and the stored position,
///   so this needs the reserver to have stopped before logging).
/// * It has, and the log holds this event or nothing: stored. (Nothing is
///   how a terminal event looks between its save and its log write, and how
///   an event whose log write failed looks.)
#[must_use]
pub fn verdict(
    reserved: &StreamResponse,
    log_seq: u64,
    position: Option<u64>,
    logged: Option<&StreamResponse>,
) -> Verdict {
    if position.is_none_or(|stored| stored < log_seq) {
        return Verdict::Release;
    }
    match logged {
        // `StreamResponse` has no `PartialEq`; both came through serde.
        Some(other) if serde_json::to_value(other).ok() != serde_json::to_value(reserved).ok() => {
            Verdict::Release
        }
        _ => Verdict::Arm,
    }
}

/// What a reservation's `log_seq` is when it reserves a task's ending, which has
/// no position in the log yet. Real positions start at 1.
pub const ENDING: u64 = 0;

/// Whether the ending reserved as `reserved` was stored: the task is stored in
/// the terminal state it names. A terminal state is never left, so this is
/// exact: another terminal state means another writer ended the task, and that
/// writer's ending is not this one.
#[must_use]
pub fn ending_verdict(
    reserved: &StreamResponse,
    stored: Option<a2a_protocol_types::task::TaskState>,
) -> Verdict {
    match (reserved, stored) {
        (StreamResponse::StatusUpdate(update), Some(state))
            if state.is_terminal() && update.status.state == state =>
        {
            Verdict::Arm
        }
        _ => Verdict::Release,
    }
}
