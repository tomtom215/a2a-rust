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
//! alike. A timestamp the executor set is kept as it is.

use a2a_protocol_types::events::StreamResponse;

/// Sets the status timestamp of a status update or a task snapshot to now,
/// when the executor left it empty.
pub(super) fn stamp_status(event: &mut StreamResponse) {
    let status = match event {
        StreamResponse::StatusUpdate(update) => &mut update.status,
        StreamResponse::Task(task) => &mut task.status,
        _ => return,
    };
    if status.timestamp.is_none() {
        status.timestamp = Some(a2a_protocol_types::utc_now_iso8601());
    }
}

#[cfg(test)]
mod tests {
    use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
    use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

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
            _ => None,
        }
    }

    #[test]
    fn a_status_without_a_timestamp_is_stamped_in_iso_8601_utc() {
        let mut event = update(TaskStatus::new(TaskState::Working));
        stamp_status(&mut event);
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
        stamp_status(&mut event);
        assert_eq!(timestamp(&event), Some("2026-01-02T03:04:05Z"));
    }
}
