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
//! A timestamp the executor set is kept when it is already in the canonical
//! UTC shape (`T`, `Z`). One with another offset, or a lower-case `t` or
//! `z`, is rewritten to the same instant in UTC, at millisecond precision:
//! §5.6.1 says timestamps MUST NOT carry an offset other than `Z`. One that
//! is not RFC 3339, as the gRPC conversion reads it, is refused, as an
//! oversized event is. Until 2026-10-10 both went out unchanged on the JSON
//! bindings, while the gRPC binding refused the malformed one at encoding,
//! and until 2026-10-13 an unpadded field or a date such as `02-30` still
//! did.

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
            // Kept as written only in the canonical shape, `T` and `Z` in
            // upper case and no offset; any other form the parser reads is
            // the same instant written canonically, so every binding serves
            // one string.
            let canonical = ts.as_bytes().get(10) == Some(&b'T') && ts.ends_with('Z');
            if !canonical {
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
        for bad in [
            "yesterday",
            "2026-1-2T3:4:5Z",
            "2026-02-30T00:00:00Z",
            " 2026-01-02T03:04:05Z",
        ] {
            let mut status = TaskStatus::new(TaskState::Completed);
            status.timestamp = Some(bad.into());
            let mut event = update(status);
            let err = stamp_status(&mut event).expect_err(bad);
            assert!(err.message.contains(bad), "{err}");
        }
    }

    /// Only the canonical shape is kept as written: a lower-case `t` or `z`
    /// is rewritten, as an offset is, so no binding serves another string.
    #[test]
    fn a_lower_case_timestamp_is_rewritten_and_a_pre_epoch_one_kept() {
        for (given, served) in [
            ("2026-01-02t03:04:05z", "2026-01-02T03:04:05.000Z"),
            ("2026-01-02T03:04:05z", "2026-01-02T03:04:05.000Z"),
            ("2026-01-02t03:04:05Z", "2026-01-02T03:04:05.000Z"),
            ("1969-07-20T20:17:00Z", "1969-07-20T20:17:00Z"),
            ("1969-07-20T21:17:00+01:00", "1969-07-20T20:17:00.000Z"),
        ] {
            let mut status = TaskStatus::new(TaskState::Completed);
            status.timestamp = Some(given.into());
            let mut event = update(status);
            stamp_status(&mut event).expect(given);
            assert_eq!(timestamp(&event), Some(served), "{given}");
        }
    }
}
