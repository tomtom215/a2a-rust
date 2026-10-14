// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Refusing an executor's event that lacks a field a2a.proto marks REQUIRED,
//! or holds what the gRPC binding cannot carry.
//!
//! A client's message is checked when it arrives; what the executor writes
//! was not. A message with no id or role, an artifact with no id, or a task
//! in `TASK_STATE_UNSPECIFIED` was stored and served, and over gRPC the last
//! reads as a task with no state (audit register R36). Checked here, on the
//! one writer every event passes through before it is persisted or
//! broadcast, as the artifact-parts check beside it already was.

use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageRole, Part, PartContent};
use a2a_protocol_types::task::{TaskState, TaskStatus};

use crate::portable::{inexact_number, standard_base64};

/// Refuses `event` when a REQUIRED field is unset: a message's `message_id`,
/// `role` or `parts`, an artifact's `artifact_id` or `parts` (an appended
/// chunk's included), or a status's `state`, in the event itself or in a
/// task snapshot's status, artifacts and history.
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub(super) fn check_required_fields(event: &StreamResponse) -> A2aResult<()> {
    match event {
        StreamResponse::Message(message) => check_message(message, "message"),
        StreamResponse::StatusUpdate(update) => check_status(&update.status),
        // a2a.proto `Artifact.parts`: "Must contain at least one part". An
        // appended chunk is an `Artifact` too; until 2026-10-14 it was let
        // through empty, and one for an artifact id the task did not hold was
        // stored as a zero-part artifact.
        StreamResponse::ArtifactUpdate(update) => {
            check_artifact_id(&update.artifact)?;
            update.artifact.validate()
        }
        StreamResponse::Task(task) => {
            check_status(&task.status)?;
            for artifact in task.artifacts.iter().flatten() {
                check_artifact_id(artifact)?;
                artifact.validate()?;
            }
            for message in task.history.iter().flatten() {
                check_message(message, "history message")?;
            }
            Ok(())
        }
        // `StreamResponse` is `#[non_exhaustive]`; an event this check does
        // not know has no field it can name.
        _ => Ok(()),
    }
}

fn check_status(status: &TaskStatus) -> A2aResult<()> {
    if status.state == TaskState::Unspecified {
        return Err(A2aError::invalid_params(
            "status.state is required: TASK_STATE_UNSPECIFIED is no state",
        ));
    }
    status
        .message
        .as_ref()
        .map_or(Ok(()), |message| check_message(message, "status message"))
}

fn check_message(message: &Message, which: &str) -> A2aResult<()> {
    if message.id.0.is_empty() {
        return Err(A2aError::invalid_params(format!(
            "{which}: messageId is required"
        )));
    }
    if message.role == MessageRole::Unspecified {
        return Err(A2aError::invalid_params(format!(
            "{which}: role is required: ROLE_USER or ROLE_AGENT"
        )));
    }
    if message.parts.is_empty() {
        return Err(A2aError::invalid_params(format!(
            "{which} must contain at least one part"
        )));
    }
    Ok(())
}

fn check_artifact_id(artifact: &Artifact) -> A2aResult<()> {
    if artifact.id.0.is_empty() {
        return Err(A2aError::invalid_params("artifactId is required"));
    }
    Ok(())
}

/// Refuses `event` when it holds what the gRPC binding cannot carry (spec §4:
/// every binding MUST represent the same thing): a part's `raw` that is not
/// base64, an integer beyond ±2^53, or a `metadata` that is not a JSON
/// object. A `raw` that is base64 is kept in the standard padded form a gRPC
/// read gives. A client's message is held to the same rules when it arrives
/// (audit register R64: an executor's events were not).
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub(super) fn make_portable(event: &mut StreamResponse) -> A2aResult<()> {
    match event {
        StreamResponse::Message(message) => portable_message(message, "message"),
        StreamResponse::StatusUpdate(update) => {
            portable_metadata(update.metadata.as_ref(), "metadata")?;
            portable_status(&mut update.status)
        }
        StreamResponse::ArtifactUpdate(update) => {
            portable_metadata(update.metadata.as_ref(), "metadata")?;
            portable_artifact(&mut update.artifact, "artifact")
        }
        StreamResponse::Task(task) => {
            portable_metadata(task.metadata.as_ref(), "metadata")?;
            portable_status(&mut task.status)?;
            for (i, artifact) in task.artifacts.iter_mut().flatten().enumerate() {
                portable_artifact(artifact, &format!("artifacts[{i}]"))?;
            }
            for (i, message) in task.history.iter_mut().flatten().enumerate() {
                portable_message(message, &format!("history[{i}]"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn portable_status(status: &mut TaskStatus) -> A2aResult<()> {
    status.message.as_mut().map_or(Ok(()), |message| {
        portable_message(message, "status.message")
    })
}

fn portable_message(message: &mut Message, at: &str) -> A2aResult<()> {
    portable_metadata(message.metadata.as_ref(), &format!("{at}.metadata"))?;
    portable_parts(&mut message.parts, at)
}

fn portable_artifact(artifact: &mut Artifact, at: &str) -> A2aResult<()> {
    portable_metadata(artifact.metadata.as_ref(), &format!("{at}.metadata"))?;
    portable_parts(&mut artifact.parts, at)
}

fn portable_parts(parts: &mut [Part], at: &str) -> A2aResult<()> {
    for (i, part) in parts.iter_mut().enumerate() {
        let field = format!("{at}.parts[{i}]");
        portable_metadata(part.metadata.as_ref(), &format!("{field}.metadata"))?;
        match &mut part.content {
            PartContent::Raw(raw) => {
                *raw = standard_base64(raw).map_err(|e| {
                    A2aError::invalid_params(format!("{field}.raw is not base64: {e}"))
                })?;
            }
            PartContent::Data(data) => exact(data, &format!("{field}.data"))?,
            _ => {}
        }
    }
    Ok(())
}

/// A `metadata` is a `google.protobuf.Struct` over gRPC: an object, and its
/// numbers exact.
fn portable_metadata(metadata: Option<&serde_json::Value>, field: &str) -> A2aResult<()> {
    match metadata {
        None => Ok(()),
        Some(value) if !value.is_object() => Err(A2aError::invalid_params(format!(
            "{field} must be a JSON object, which is all a gRPC google.protobuf.Struct holds"
        ))),
        Some(value) => exact(value, field),
    }
}

fn exact(value: &serde_json::Value, field: &str) -> A2aResult<()> {
    inexact_number(value).map_or(Ok(()), |n| {
        Err(A2aError::invalid_params(format!(
            "{field} holds the integer {n}, beyond the ±2^53 a protobuf number holds exactly"
        )))
    })
}
