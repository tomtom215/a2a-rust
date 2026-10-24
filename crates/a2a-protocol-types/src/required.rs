// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The REQUIRED fields of `a2a.proto` that neither wire format's parser
//! enforces, checked the same way at both ends.
//!
//! Spec §5.7: a field marked REQUIRED "MUST be present and set in valid
//! messages"; implementations "SHOULD validate these requirements and reject
//! messages with missing required fields", and a required array "MUST
//! contain at least one element". The JSON bindings refuse a missing key
//! when they parse, and the gRPC conversion (module `proto`, feature
//! `proto`) refuses the absent field proto3 reads as `""` or `0`. What both
//! still read as a value is checked here: an empty string sent explicitly,
//! an empty required array (`ProtoJSON` omits an empty repeated field, so
//! neither format can refuse its absence at parse time), and over JSON an
//! explicit `TASK_STATE_UNSPECIFIED` or `ROLE_UNSPECIFIED`.
//!
//! - The server runs [`check_event`] on every event an executor writes,
//!   before it is stored or sent (until 2026-10-23 this lived in the server
//!   crate's event queue).
//! - The client runs [`check_task`], [`check_received_event`],
//!   [`check_response`], [`check_push_config`] and [`check_card`] on what it
//!   receives, whichever binding carried it.
//!
//! Every check answers `InvalidParams` naming the field, as the server's
//! refusal of the same object does; a client reports it as the agent's
//! invalid response.

use crate::agent_card::AgentCard;
use crate::artifact::Artifact;
use crate::error::{A2aError, A2aResult};
use crate::events::StreamResponse;
use crate::message::{Message, MessageRole};
use crate::push::TaskPushNotificationConfig;
use crate::responses::SendMessageResponse;
use crate::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

/// Refuses `event` when a REQUIRED field of its content is unset.
///
/// Those are a message's `message_id`, `role` or `parts`, an artifact's
/// `artifact_id` or `parts` (an appended chunk's included), and a status's
/// `state`, in the event itself or in a task snapshot's status, artifacts
/// and history.
///
/// The ids that name the task an event belongs to are not checked: a server
/// stamps them on its executor's events. [`check_received_event`] checks
/// them as well.
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub fn check_event(event: &StreamResponse) -> A2aResult<()> {
    match event {
        StreamResponse::Message(message) => check_message(message, "message"),
        StreamResponse::StatusUpdate(update) => check_status(&update.status),
        // a2a.proto `Artifact.parts`: "Must contain at least one part". An
        // appended chunk is an `Artifact` too.
        StreamResponse::ArtifactUpdate(update) => check_artifact(&update.artifact),
        StreamResponse::Task(task) => check_task_content(task),
    }
}

/// [`check_event`], and the REQUIRED ids an event carries: a task's `id`,
/// and a status or artifact update's `task_id` and `context_id`. What a
/// client runs on each streamed event.
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub fn check_received_event(event: &StreamResponse) -> A2aResult<()> {
    match event {
        StreamResponse::StatusUpdate(update) => {
            check_event_ids(&update.task_id, &update.context_id, "statusUpdate")?;
        }
        StreamResponse::ArtifactUpdate(update) => {
            check_event_ids(&update.task_id, &update.context_id, "artifactUpdate")?;
        }
        StreamResponse::Task(task) => check_task_id(task)?,
        StreamResponse::Message(_) => {}
    }
    check_event(event)
}

/// Refuses a task with no `id`, or whose status, artifacts or history lack
/// a REQUIRED field (see [`check_event`]).
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub fn check_task(task: &Task) -> A2aResult<()> {
    check_task_id(task)?;
    check_task_content(task)
}

/// [`check_task`] or [`check_message`], for what a blocking `SendMessage`
/// returned.
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub fn check_response(response: &SendMessageResponse) -> A2aResult<()> {
    match response {
        SendMessageResponse::Task(task) => check_task(task),
        SendMessageResponse::Message(message) => check_message(message, "message"),
    }
}

/// Refuses a push-notification config with no webhook `url`, or whose
/// `authentication` has no `scheme`.
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub fn check_push_config(config: &TaskPushNotificationConfig) -> A2aResult<()> {
    if config.url.is_empty() {
        return Err(A2aError::invalid_params(
            "push notification config: url is required",
        ));
    }
    if config
        .authentication
        .as_ref()
        .is_some_and(|auth| auth.scheme.is_empty())
    {
        return Err(A2aError::invalid_params(
            "push notification config: authentication.scheme is required",
        ));
    }
    Ok(())
}

/// Refuses an agent card [`AgentCard::validate`] or
/// [`AgentCard::validate_security_schemes`] refuses: the rules a server
/// applies before it publishes one.
///
/// # Errors
///
/// `InvalidParams` with the rule the card breaks.
pub fn check_card(card: &AgentCard) -> A2aResult<()> {
    card.validate()
        .and_then(|()| card.validate_security_schemes())
        .map_err(A2aError::invalid_params)
}

/// Refuses a status in `TASK_STATE_UNSPECIFIED`, or whose message lacks a
/// REQUIRED field.
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub fn check_status(status: &TaskStatus) -> A2aResult<()> {
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

/// Refuses a message with no `message_id`, a `role` of `ROLE_UNSPECIFIED`,
/// or no parts; `which` names it in the error ("history message").
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub fn check_message(message: &Message, which: &str) -> A2aResult<()> {
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

/// Refuses an artifact with no `artifact_id` or no parts.
///
/// # Errors
///
/// `InvalidParams` naming the field.
pub fn check_artifact(artifact: &Artifact) -> A2aResult<()> {
    if artifact.id.0.is_empty() {
        return Err(A2aError::invalid_params("artifactId is required"));
    }
    artifact.validate()
}

fn check_task_content(task: &Task) -> A2aResult<()> {
    check_status(&task.status)?;
    for artifact in task.artifacts.iter().flatten() {
        check_artifact(artifact)?;
    }
    for message in task.history.iter().flatten() {
        check_message(message, "history message")?;
    }
    Ok(())
}

fn check_task_id(task: &Task) -> A2aResult<()> {
    if task.id.0.is_empty() {
        return Err(A2aError::invalid_params("task: id is required"));
    }
    Ok(())
}

fn check_event_ids(task_id: &TaskId, context_id: &ContextId, which: &str) -> A2aResult<()> {
    if task_id.0.is_empty() {
        return Err(A2aError::invalid_params(format!(
            "{which}: taskId is required"
        )));
    }
    if context_id.0.is_empty() {
        return Err(A2aError::invalid_params(format!(
            "{which}: contextId is required"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
