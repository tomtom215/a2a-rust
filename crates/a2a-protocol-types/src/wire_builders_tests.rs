// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The builders of the `#[non_exhaustive]` protocol objects: each sets its own
//! field and no other, and a built value is the value the literal it replaces
//! would have been (the crate can still write literals; callers cannot).

use serde_json::json;

use super::*;
use crate::message::{MessageRole, Part};
use crate::task::TaskState;

fn msg() -> Message {
    Message::new("m1", MessageRole::Agent, vec![Part::text("hi")])
}

fn json_of<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("serializes")
}

#[test]
fn task_status_sets_its_message_and_its_timestamp_and_nothing_else() {
    let status = TaskStatus::new(TaskState::Failed)
        .with_message(msg())
        .at("2026-11-01T00:00:00Z");
    assert_eq!(status.state, TaskState::Failed);
    assert_eq!(status.message, Some(msg()));
    assert_eq!(status.timestamp.as_deref(), Some("2026-11-01T00:00:00Z"));

    let unset = TaskStatus::new(TaskState::Working);
    assert_eq!((unset.message, unset.timestamp), (None, None));
}

#[test]
fn task_builders_set_history_artifacts_and_metadata() {
    let artifact = Artifact::new("a", vec![Part::text("x")]);
    let task = Task::new("t", "c", TaskStatus::new(TaskState::Completed))
        .with_history(vec![msg()])
        .with_artifacts(vec![artifact.clone()])
        .with_metadata(json!({"k": 1}));
    assert_eq!(task.history, Some(vec![msg()]));
    assert_eq!(task.artifacts, Some(vec![artifact]));
    assert_eq!(task.metadata, Some(json!({"k": 1})));

    let unset = Task::new("t", "c", TaskStatus::new(TaskState::Working));
    assert_eq!(
        (unset.history, unset.artifacts, unset.metadata),
        (None, None, None)
    );
}

#[test]
fn artifact_builders_each_set_one_field() {
    let base = || Artifact::new("a", vec![Part::text("x")]);
    assert_eq!(base().with_name("n").name.as_deref(), Some("n"));
    assert_eq!(
        base().with_description("d").description.as_deref(),
        Some("d")
    );
    assert_eq!(base().with_metadata(json!(1)).metadata, Some(json!(1)));
    assert_eq!(
        base().with_extensions(["urn:x", "urn:y"]).extensions,
        Some(vec!["urn:x".to_owned(), "urn:y".to_owned()])
    );
    // And the others stay unset.
    let named = base().with_name("n");
    assert_eq!(
        (named.description, named.metadata, named.extensions),
        (None, None, None)
    );
}

/// `Artifact::new` refuses an empty artifact in a debug build; `unchecked`
/// is how a test builds the one `validate` is meant to refuse.
#[test]
fn an_artifact_with_no_parts_can_be_built_to_test_that_it_is_refused() {
    let artifact = Artifact::unchecked("a", vec![]);
    assert_eq!(artifact.parts, Vec::<Part>::new());
    assert!(artifact.validate().is_err());
    assert_eq!(artifact.id.to_string(), "a");
}

#[test]
fn status_update_event_new_carries_the_three_required_fields() {
    let event = TaskStatusUpdateEvent::new("t", "c", TaskStatus::new(TaskState::Working));
    assert_eq!(event.task_id.to_string(), "t");
    assert_eq!(event.context_id.to_string(), "c");
    assert_eq!(event.status.state, TaskState::Working);
    assert_eq!(event.metadata, None);
    assert_eq!(
        event.with_metadata(json!({"a": 1})).metadata,
        Some(json!({"a": 1}))
    );
}

#[test]
fn artifact_update_event_sets_append_last_chunk_and_metadata() {
    let artifact = Artifact::new("a", vec![Part::text("x")]);
    let event = TaskArtifactUpdateEvent::new("t", "c", artifact.clone());
    assert_eq!(
        (event.append, event.last_chunk, event.metadata.clone()),
        (None, None, None)
    );
    assert_eq!(event.artifact, artifact);

    let event = event
        .with_append(true)
        .with_last_chunk(false)
        .with_metadata(json!(2));
    assert_eq!(event.append, Some(true));
    assert_eq!(event.last_chunk, Some(false));
    assert_eq!(event.metadata, Some(json!(2)));
}

/// The same value a literal wrote, on the wire: `append: false` is written
/// out, not dropped, which is how a client tells replace from "not stated".
#[test]
fn a_built_event_serialises_as_its_literal_did() {
    let built = TaskArtifactUpdateEvent::new("t", "c", Artifact::new("a", vec![Part::text("x")]))
        .with_append(false);
    let literal = TaskArtifactUpdateEvent {
        task_id: "t".into(),
        context_id: "c".into(),
        artifact: Artifact::new("a", vec![Part::text("x")]),
        append: Some(false),
        last_chunk: None,
        metadata: None,
    };
    assert_eq!(json_of(&built), json_of(&literal));
    assert_eq!(json_of(&built)["append"], json!(false));
}

#[test]
fn push_config_builders_each_set_one_field() {
    let base = || TaskPushNotificationConfig::new("t", "https://hooks.example/x");
    assert_eq!(base().with_id("i").id.as_deref(), Some("i"));
    assert_eq!(base().with_token("tok").token.as_deref(), Some("tok"));
    assert_eq!(base().with_tenant("acme").tenant.as_deref(), Some("acme"));
    let auth = AuthenticationInfo {
        scheme: "Bearer".into(),
        credentials: Some("c".into()),
    };
    let with_auth = base().with_authentication(auth);
    assert_eq!(
        with_auth
            .authentication
            .as_ref()
            .map(|a| (a.scheme.as_str(), a.credentials.as_deref())),
        Some(("Bearer", Some("c")))
    );

    let unset = base();
    assert_eq!((unset.id, unset.token, unset.tenant), (None, None, None));
    assert!(unset.authentication.is_none());
}

/// A config with no task: the id the request will carry is not known yet.
#[test]
fn a_config_for_a_url_names_no_task() {
    let config = TaskPushNotificationConfig::for_url("https://hooks.example/x");
    assert_eq!(config.task_id, None);
    assert_eq!(config.url, "https://hooks.example/x");
    assert_eq!((config.id, config.token, config.tenant), (None, None, None));
}

#[test]
fn agent_extension_builders_each_set_one_field() {
    let base = || AgentExtension::new("urn:x");
    assert_eq!(
        base().with_description("d").description.as_deref(),
        Some("d")
    );
    assert_eq!(base().with_required(true).required, Some(true));
    assert_eq!(
        base().with_params(json!({"p": 1})).params,
        Some(json!({"p": 1}))
    );
    let unset = base();
    assert_eq!(
        (unset.description, unset.required, unset.params),
        (None, None, None)
    );
}

#[test]
fn provider_capabilities_and_interface_builders() {
    let provider = AgentProvider::new("Acme", "https://acme.example");
    assert_eq!(provider.organization, "Acme");
    assert_eq!(provider.url, "https://acme.example");

    let caps = AgentCapabilities::none().with_extensions(vec![AgentExtension::new("urn:x")]);
    assert_eq!(caps.extensions.as_ref().map(Vec::len), Some(1));
    assert_eq!(caps.streaming, None);

    let iface = AgentInterface::jsonrpc("http://x").with_protocol_version("0.3");
    assert_eq!(iface.protocol_version, "0.3");
    assert_eq!(iface.protocol_binding, "JSONRPC");
}
