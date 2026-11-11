// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What settles a reservation whose reserver stopped: every row of the table
//! in [`verdict`]'s documentation, one case each.

use a2a_protocol_types::artifact::{Artifact, ArtifactId};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use super::{Verdict, may_push, verdict};

fn status(state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t1"),
        ContextId::new("c1"),
        TaskStatus::new(state),
    ))
}

#[test]
fn a_task_that_has_not_reached_the_position_did_not_store_the_event() {
    let event = status(TaskState::Working);
    // Whether or not the log has it: the log is written before the fold.
    assert_eq!(verdict(&event, 5, Some(4), Some(&event)), Verdict::Release);
    assert_eq!(verdict(&event, 5, Some(4), None), Verdict::Release);
    assert_eq!(verdict(&event, 5, Some(0), None), Verdict::Release);
}

#[test]
fn a_task_with_no_position_did_not_store_the_event() {
    let event = status(TaskState::Working);
    assert_eq!(verdict(&event, 5, None, None), Verdict::Release);
    assert_eq!(verdict(&event, 5, None, Some(&event)), Verdict::Release);
}

#[test]
fn a_task_at_the_position_with_the_event_in_its_log_stored_it() {
    let event = status(TaskState::Working);
    assert_eq!(verdict(&event, 5, Some(5), Some(&event)), Verdict::Arm);
    // Later events were folded too.
    assert_eq!(verdict(&event, 5, Some(9), Some(&event)), Verdict::Arm);
}

/// A terminal event is logged after the fold, and an event whose log write
/// failed is never logged: the stored position is the evidence.
#[test]
fn a_task_at_the_position_with_nothing_in_the_log_stored_it() {
    let end = status(TaskState::Completed);
    assert_eq!(verdict(&end, 5, Some(5), None), Verdict::Arm);
}

/// The reserver stopped before logging, and the next turn took the position:
/// the task reflects it, but not this event.
#[test]
fn a_position_taken_by_another_event_is_not_this_one() {
    let reserved = status(TaskState::Working);
    let other = status(TaskState::Completed);
    assert_eq!(
        verdict(&reserved, 5, Some(5), Some(&other)),
        Verdict::Release
    );
    assert_eq!(
        verdict(&reserved, 5, Some(7), Some(&other)),
        Verdict::Release
    );
}

/// Two artifact updates of one task that differ only in their parts are
/// different events.
#[test]
fn events_that_differ_in_content_are_different() {
    let artifact = |text: &str| {
        StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent::new(
            TaskId::new("t1"),
            ContextId::new("c1"),
            Artifact::new(ArtifactId::new("a"), vec![Part::text(text)]),
        ))
    };
    assert_eq!(
        verdict(&artifact("one"), 3, Some(3), Some(&artifact("one"))),
        Verdict::Arm
    );
    assert_eq!(
        verdict(&artifact("one"), 3, Some(3), Some(&artifact("two"))),
        Verdict::Release
    );
}

/// A snapshot or a message is never pushed, so it is never reserved.
#[test]
fn only_status_and_artifact_updates_are_reserved() {
    assert!(may_push(&status(TaskState::Working)));
    assert!(!may_push(&StreamResponse::Message(
        a2a_protocol_types::message::Message::user("m1", vec![Part::text("hi")])
    )));
}
