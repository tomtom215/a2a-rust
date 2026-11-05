// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Compaction of a live task's event log to its stored snapshot (audit H13,
//! ADR 0047, option C), driven by
//! [`RetentionPolicy::max_events_per_live_task`](super::RetentionPolicy::max_events_per_live_task).
//!
//! A live task's log otherwise grows for as long as the task runs: rows go
//! only when a terminal task is purged. Compaction keeps the newest `keep`
//! events and drops the rest, but only events the stored snapshot already
//! reflects, so nothing is lost that the task row does not hold. A reader
//! that later asks to resume from before the cut is told the log no longer
//! covers it and is answered with the snapshot, as a lagged reader is.

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::task::TaskId;

use crate::store::TaskStore;

/// Where a live task's log is cut: below the oldest of the newest `keep`
/// events, never past the snapshot (`applied`, the stored task's position),
/// and never the last event, whose position a continued task numbers on
/// from. A `keep` of 0 is taken as 1 for that reason.
pub fn trim_point(last: u64, applied: u64, keep: u64) -> u64 {
    let newest = last.saturating_sub(keep.max(1)).saturating_add(1);
    newest.min(applied.saturating_add(1))
}

/// Compacts one task's log to the newest `keep` events its snapshot
/// reflects, returning how many events were dropped. A task that is gone,
/// terminal (retention purges it whole), or stored without a position (the
/// snapshot's coverage is unknown) is left alone.
pub async fn compact_task<S: TaskStore + ?Sized>(
    store: &S,
    task_id: &TaskId,
    keep: u64,
) -> A2aResult<u64> {
    let Some((task, Some(applied))) = store.get_with_position(task_id).await? else {
        return Ok(0);
    };
    if task.status.state.is_terminal() {
        return Ok(0);
    }
    let last = store.last_event_seq(task_id).await?;
    store
        .trim_event_log(task_id, trim_point(last, applied, keep))
        .await
}

/// The count a log must exceed to be compacted, as a SQL parameter.
pub fn over(keep: u64) -> i64 {
    i64::try_from(keep.max(1)).unwrap_or(i64::MAX)
}
