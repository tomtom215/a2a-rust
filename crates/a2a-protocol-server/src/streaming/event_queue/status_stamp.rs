// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Giving every task status a timestamp before it leaves the executor.
//!
//! Spec §5.6.1 says timestamps are ISO 8601 UTC strings, and §3.1.4 orders
//! `ListTasks` by each task's status timestamp, so a status without one can
//! neither be read as "when" nor be ordered. `TaskStatus::new` leaves the
//! field empty, and until 2026-09-25 this crate's own `EventEmitter::status`
//! used it: every agent built on the recommended helper served statuses with
//! no timestamp, and failed ACTS DM-SERIAL-001 (a MUST).
//!
//! Stamping here, on the one writer every event passes through before it is
//! persisted or broadcast, covers the helper and hand-written executors
//! alike.
//!
//! A timestamp the executor set is kept when it is already UTC (`Z`). One
//! with another offset is rewritten to the same instant in UTC, at
//! millisecond precision: §5.6.1 says timestamps MUST NOT carry an offset
//! other than `Z`. One that does not parse is refused, as an oversized event
//! is. Until 2026-10-10 both went out unchanged on the JSON bindings, while
//! the gRPC binding refused the malformed one at encoding.

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;

/// Gives the status of a status update or a task snapshot a UTC timestamp:
/// now when the executor left it empty, the same instant in UTC when it
/// carries another offset.
///
/// # Errors
///
/// An internal error when the executor set a timestamp that is not ISO 8601.
pub(super) fn stamp_status(event: &mut StreamResponse) -> A2aResult<()> {
    let status = match event {
        StreamResponse::StatusUpdate(update) => &mut update.status,
        StreamResponse::Task(task) => &mut task.status,
        _ => return Ok(()),
    };
    match status.timestamp.as_deref() {
        None => status.timestamp = Some(a2a_protocol_types::utc_now_iso8601()),
        Some(ts) => {
            let millis = a2a_protocol_types::parse_iso8601_to_unix_millis(ts).ok_or_else(|| {
                A2aError::internal(format!(
                    "status timestamp {ts:?} is not an ISO 8601 date-time (spec §5.6.1)"
                ))
            })?;
            if !ts.trim_end().ends_with(['Z', 'z']) {
                status.timestamp = Some(a2a_protocol_types::unix_millis_to_iso8601(millis));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
    use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

    use super::stamp_status;

    fn update(status: TaskStatus) -> StreamResponse {
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new("t"),
            context_id: ContextId::new("c"),
            status,
            metadata: None,
        })
    }

    fn timestamp(event: &StreamResponse) -> Option<&str> {
        match event {
            StreamResponse::StatusUpdate(u) => u.status.timestamp.as_deref(),
            StreamResponse::Task(t) => t.status.timestamp.as_deref(),
            _ => None,
        }
    }

    #[test]
    fn a_status_without_a_timestamp_is_stamped_in_iso_8601_utc() {
        let mut event = update(TaskStatus::new(TaskState::Working));
        stamp_status(&mut event).expect("stamps");
        let ts = timestamp(&event).expect("stamped");
        assert!(ts.ends_with('Z') && ts.contains('T'), "{ts}");
        assert!(
            a2a_protocol_types::parse_iso8601_to_unix_millis(ts).is_some(),
            "{ts} parses"
        );
    }

    #[test]
    fn a_timestamp_the_executor_set_is_kept() {
        let mut status = TaskStatus::new(TaskState::Completed);
        status.timestamp = Some("2026-01-02T03:04:05Z".into());
        let mut event = update(status);
        stamp_status(&mut event).expect("stamps");
        assert_eq!(timestamp(&event), Some("2026-01-02T03:04:05Z"));
    }

    #[test]
    fn a_task_snapshot_without_a_timestamp_is_stamped() {
        let mut event = StreamResponse::Task(Task {
            id: TaskId::new("t"),
            context_id: ContextId::new("c"),
            status: TaskStatus::new(TaskState::Submitted),
            history: None,
            artifacts: None,
            metadata: None,
        });
        stamp_status(&mut event).expect("stamps");
        let ts = timestamp(&event).expect("stamped");
        assert!(
            a2a_protocol_types::parse_iso8601_to_unix_millis(ts).is_some(),
            "{ts} parses"
        );
    }

    /// §5.6.1: no offset other than `Z`. The same instant, in UTC.
    #[test]
    fn an_offset_timestamp_is_rewritten_in_utc() {
        let mut status = TaskStatus::new(TaskState::Completed);
        status.timestamp = Some("2026-01-02T05:04:05+02:00".into());
        let mut event = update(status);
        stamp_status(&mut event).expect("stamps");
        assert_eq!(timestamp(&event), Some("2026-01-02T03:04:05.000Z"));
    }

    #[test]
    fn a_timestamp_that_is_not_iso_8601_is_refused() {
        let mut status = TaskStatus::new(TaskState::Completed);
        status.timestamp = Some("yesterday".into());
        let mut event = update(status);
        let err = stamp_status(&mut event).expect_err("refused");
        assert!(err.message.contains("yesterday"), "{err}");
    }
}
