// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Shared helpers used across the agent team example.

use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;

/// Creates a simple [`MessageSendParams`] with a single text part.
pub fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        context_id: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }
}

/// Re-export [`EventEmitter`] from the SDK for backwards compatibility.
///
/// This was originally defined here as a dogfood finding; it has now been
/// upstreamed to `a2a_protocol_server::executor_helpers::EventEmitter`.
pub use a2a_protocol_server::executor_helpers::EventEmitter;
