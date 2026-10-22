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
