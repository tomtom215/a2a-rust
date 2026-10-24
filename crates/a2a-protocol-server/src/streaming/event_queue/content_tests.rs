// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What `make_portable` leaves of fields protobuf cannot tell from absent.

use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent};
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::task::{ContextId, TaskId};

use super::make_portable;

/// An empty list or string is sent as absent, as gRPC would read it back.
/// The mutation gate found the lists untested: keeping them survived.
#[test]
fn empty_lists_and_names_are_unset() {
    let mut message = Message::user_text("m-1", "hi");
    message.extensions = Some(Vec::new());
    message.reference_task_ids = Some(Vec::new());
    let mut event = StreamResponse::Message(message);
    make_portable(&mut event).unwrap();
    let StreamResponse::Message(message) = event else {
        unreachable!("a message stays a message")
    };
    assert_eq!(message.extensions, None);
    assert_eq!(message.reference_task_ids, None);

    let mut artifact = Artifact::new("a-1", vec![Part::text("x")]);
    artifact.extensions = Some(Vec::new());
    artifact.name = Some(String::new());
    let mut event = StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
        task_id: TaskId::new("t-1"),
        context_id: ContextId::new("c-1"),
        artifact,
        append: None,
        last_chunk: None,
        metadata: None,
    });
    make_portable(&mut event).unwrap();
    let StreamResponse::ArtifactUpdate(update) = event else {
        unreachable!("an update stays an update")
    };
    assert_eq!(update.artifact.extensions, None);
    assert_eq!(update.artifact.name, None);
}

/// Provenance that is not provenance is refused wherever an executor's event
/// carries content: a message, a status update's message, and a task's
/// status message, artifacts and history. The mutation gate found the
/// message, status and task arms untested: deleting each survived.
#[test]
fn malformed_provenance_is_refused_in_every_place_content_rides() {
    use a2a_protocol_types::events::TaskStatusUpdateEvent;
    use a2a_protocol_types::provenance::PROVENANCE_METADATA_KEY;
    use a2a_protocol_types::task::{Task, TaskState, TaskStatus};

    use super::check_required_fields;

    let message = |bad: bool| {
        let mut m = Message::agent_text("m-1", "hi");
        if bad {
            m.metadata = Some(serde_json::json!({ PROVENANCE_METADATA_KEY: "not provenance" }));
        }
        m
    };
    let artifact = |bad: bool| {
        let mut a = Artifact::new("a-1", vec![Part::text("x")]);
        if bad {
            a.metadata = Some(serde_json::json!({ PROVENANCE_METADATA_KEY: 7 }));
        }
        a
    };
    let status = |bad: bool| {
        let mut s = TaskStatus::new(TaskState::Working);
        s.message = Some(message(bad));
        s
    };
    let update = |bad: bool| {
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new("t-1"),
            context_id: ContextId::new("c-1"),
            status: status(bad),
            metadata: None,
        })
    };
    let task = |status_bad: bool, artifact_bad: bool, history_bad: bool| {
        let mut t = Task::new("t-1", "c-1", status(status_bad));
        t.artifacts = Some(vec![artifact(artifact_bad)]);
        t.history = Some(vec![message(history_bad)]);
        StreamResponse::Task(t)
    };

    for good in [
        StreamResponse::Message(message(false)),
        update(false),
        task(false, false, false),
    ] {
        assert!(check_required_fields(&good).is_ok(), "{good:?}");
    }
    for bad in [
        StreamResponse::Message(message(true)),
        update(true),
        task(true, false, false),
        task(false, true, false),
        task(false, false, true),
    ] {
        let err = check_required_fields(&bad).expect_err("refused");
        assert!(err.message.contains("malformed provenance"), "{err:?}");
    }
}
