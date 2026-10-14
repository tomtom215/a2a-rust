// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Refusing an executor's event that lacks a field a2a.proto marks REQUIRED.
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
use a2a_protocol_types::message::{Message, MessageRole};
use a2a_protocol_types::task::{TaskState, TaskStatus};

/// Refuses `event` when a REQUIRED field is unset: a message's `message_id`,
/// `role` or `parts`, an artifact's `artifact_id` or `parts` (an appended
/// chunk may have none), or a status's `state`, in the event itself or in a
/// task snapshot's status, artifacts and history.
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub(super) fn check_required_fields(event: &StreamResponse) -> A2aResult<()> {
    match event {
        StreamResponse::Message(message) => check_message(message, "message"),
        StreamResponse::StatusUpdate(update) => check_status(&update.status),
        StreamResponse::ArtifactUpdate(update) => {
            check_artifact_id(&update.artifact)?;
            if update.append == Some(true) {
                Ok(())
            } else {
                update.artifact.validate()
            }
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
