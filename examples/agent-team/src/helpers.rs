// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Shared helpers used across the agent team example.

use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;

/// Creates a simple [`MessageSendParams`] with a single text part.
pub fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
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

/// Reads from `stream` until an event carrying a task id arrives, and returns
/// that id.
///
/// **Why this exists.** The first event on a streaming send is a full
/// [`StreamResponse::Task`](a2a_protocol_types::events::StreamResponse::Task)
/// snapshot, not a status update — verified against a
/// live server, where the order is `task` → `artifactUpdate` → `statusUpdate`.
/// Three tests here each hand-rolled `if let Some(Ok(StatusUpdate(ev)))` on the
/// *first* event, so all three read the snapshot, failed to match, and reported
/// "no task_id from stream". They had never passed.
///
/// Every variant except
/// [`StreamResponse::Message`](a2a_protocol_types::events::StreamResponse::Message)
/// carries a task id, so this
/// keeps reading rather than assuming any particular arrival order — the same
/// correction already applied to `BIND-EQUIV-003` in the in-repo TCK.
///
/// Returns `None` only if the stream ends without ever naming a task, or after
/// `max_events` (a bound so a misbehaving server cannot hang the suite).
pub async fn first_task_id(
    stream: &mut a2a_protocol_client::streaming::EventStream,
    max_events: usize,
) -> Option<String> {
    use a2a_protocol_types::events::StreamResponse;

    for _ in 0..max_events {
        match stream.next().await {
            Some(Ok(StreamResponse::Task(t))) => return Some(t.id.0.clone()),
            Some(Ok(StreamResponse::StatusUpdate(ev))) => return Some(ev.task_id.0.clone()),
            Some(Ok(StreamResponse::ArtifactUpdate(ev))) => return Some(ev.task_id.0.clone()),
            // A lag signal is recoverable — the task id may still be coming.
            Some(Err(e)) if e.is_stream_lagged() => {}
            Some(Err(_)) | None => return None,
            // A bare `Message` carries no task, and `StreamResponse` is
            // `#[non_exhaustive]`, so any future variant lands here too: keep
            // reading rather than concluding there is no task.
            Some(Ok(_)) => {}
        }
    }
    None
}
