// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Holding what an agent sends to `a2a.proto`'s REQUIRED fields (spec §5.7:
//! implementations "SHOULD validate these requirements and reject messages
//! with missing required fields").
//!
//! One rule set, [`a2a_protocol_types::required`], which the server applies
//! to what its executors write; here it is applied where each received
//! object enters the client, after the transport has decoded it, so every
//! binding is held to the same rules: the tasks, pages and replies
//! `get_task`, `list_tasks`, `cancel_task` and `send_message` return, each
//! streamed event, the push configs the push-config calls return, and the
//! agent card discovery and `GetExtendedAgentCard` fetch. Over gRPC a field
//! absent on the wire is refused earlier, by the conversion, with the same
//! error. Until 2026-10-23 none of it was checked: a task with no id or
//! state, or a card with no skills, was returned as if valid.

use a2a_protocol_types::error::A2aError;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::required;
use a2a_protocol_types::responses::{SendMessageResponse, TaskListResponse};
use a2a_protocol_types::{AgentCard, Task};

use crate::error::{ClientError, ClientResult};

/// Refuses a task `method` returned that lacks a REQUIRED field.
pub fn task(method: &str, task: &Task) -> ClientResult<()> {
    required::check_task(task).map_err(|e| refused(&format!("{method} response"), &e))
}

/// Refuses a `ListTasks` page holding such a task.
pub fn page(page: &TaskListResponse) -> ClientResult<()> {
    page.tasks
        .iter()
        .try_for_each(|t| required::check_task(t).map_err(|e| refused("ListTasks response", &e)))
}

/// Refuses what a blocking `SendMessage` returned, task or message, that
/// lacks a REQUIRED field.
pub fn response(response: &SendMessageResponse) -> ClientResult<()> {
    required::check_response(response).map_err(|e| refused("SendMessage response", &e))
}

/// Refuses a streamed event that lacks a REQUIRED field, its ids included.
pub fn event(event: &StreamResponse) -> ClientResult<()> {
    required::check_received_event(event).map_err(|e| refused("streamed event", &e))
}

/// Refuses a push config `method` returned with no `url`, or no
/// `authentication.scheme`.
pub fn push_config(method: &str, config: &TaskPushNotificationConfig) -> ClientResult<()> {
    required::check_push_config(config).map_err(|e| refused(&format!("{method} response"), &e))
}

/// Refuses an agent card, fetched from `source`, that a server would refuse
/// to publish ([`AgentCard::validate`] and
/// [`AgentCard::validate_security_schemes`]).
pub fn card(source: &str, card: &AgentCard) -> ClientResult<()> {
    required::check_card(card).map_err(|e| {
        ClientError::InvalidAgentResponse(format!(
            "the agent card from {source} is not a valid A2A card: {} \
             (a2a.proto field rules, spec §5.7; a server refuses to publish it)",
            e.message
        ))
    })
}

fn refused(from: &str, e: &A2aError) -> ClientError {
    ClientError::InvalidAgentResponse(format!(
        "{from}: {} (a2a.proto marks the field REQUIRED; spec §5.7)",
        e.message
    ))
}
