// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Ingress validation of a `SendMessage` request: ids, parts and metadata.
//!
//! Runs before anything with a side effect, so a rejected request leaves no
//! task row, queue or token behind. The checks are in the order the errors
//! were always reported in, which matters to a client that fixes one problem
//! at a time.

use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::MessageSendParams;

use super::super::RequestHandler;
use super::super::helpers::{validate_id, validate_metadata_object};
use super::decisions::json_byte_len;
use crate::error::{ServerError, ServerResult};

impl RequestHandler {
    /// Validates the request in place: proto3-empty ids are unset, then ids,
    /// parts and every `metadata` field are checked against the handler's
    /// limits.
    ///
    /// # Errors
    ///
    /// [`ServerError::InvalidParams`] naming the first field that failed.
    pub(super) fn validate_send_params(&self, params: &mut MessageSendParams) -> ServerResult<()> {
        unset_proto3_empty_ids(&mut params.message);

        // Validate incoming IDs: reject empty/whitespace-only and excessively
        // long values (AP-1).
        if let Some(ref ctx_id) = params.message.context_id {
            validate_id(&ctx_id.0, "context_id", self.limits.max_id_length)?;
        }
        if let Some(ref task_id) = params.message.task_id {
            validate_id(&task_id.0, "task_id", self.limits.max_id_length)?;
        }

        // SC-4: Reject messages with no parts.
        if params.message.parts.is_empty() {
            return Err(ServerError::InvalidParams(
                "message must contain at least one part".into(),
            ));
        }

        // Cross-binding portability: every client-supplied `metadata` field
        // must be a JSON object so the resulting task is representable over
        // gRPC (google.protobuf.Struct), not just over JSON-RPC/REST. Reject
        // arrays and scalars at ingress rather than storing a task that one
        // binding can serve and another cannot.
        validate_metadata_object(params.message.metadata.as_ref(), "message")?;
        validate_metadata_object(params.metadata.as_ref(), "request")?;
        for (i, part) in params.message.parts.iter().enumerate() {
            validate_metadata_object(part.metadata.as_ref(), &format!("message part {i}"))?;
        }

        // PR-8: Reject oversized metadata to prevent memory exhaustion.
        let max_meta = self.limits.max_metadata_size;
        check_metadata_size(params.message.metadata.as_ref(), "message", max_meta)?;
        check_metadata_size(params.metadata.as_ref(), "request", max_meta)
    }
}

/// Rejects a `metadata` object whose JSON encoding exceeds `max_meta` bytes.
///
/// Uses a byte-counting writer rather than serializing to a throwaway
/// `String`, so measuring the limit costs no allocation. `what` names the
/// field in the error — `"message"` or `"request"`.
fn check_metadata_size(
    metadata: Option<&serde_json::Value>,
    what: &str,
    max_meta: usize,
) -> ServerResult<()> {
    let Some(meta) = metadata else {
        return Ok(());
    };
    let meta_size = json_byte_len(meta)
        .map_err(|_| ServerError::InvalidParams(format!("{what} metadata is not serializable")))?;
    if meta_size > max_meta {
        return Err(ServerError::InvalidParams(format!(
            "{what} metadata exceeds maximum size ({meta_size} bytes, max {max_meta})"
        )));
    }
    Ok(())
}

/// Maps an exactly-empty `contextId` / `taskId` on an incoming message to
/// absent.
///
/// The A2A JSON bindings are `ProtoJSON`, and both fields are proto3 strings
/// without presence: an empty string *is* the unset value. A client that
/// prints every field — a2a-java's JSON-RPC transport uses
/// `alwaysPrintFieldsWithNoPresence` — therefore sends `"contextId": ""` for
/// "none", and until 2026-09-09 this server rejected that as an invalid id,
/// failing every JSON-RPC pairing with the Java SDK in the official ITK while
/// the same peer passed over gRPC and HTTP+JSON, whose printers omit
/// defaults. Whitespace-only is left alone and still rejected by
/// `validate_id`: no printer produces it for "unset".
fn unset_proto3_empty_ids(message: &mut Message) {
    if message.context_id.as_ref().is_some_and(|c| c.0.is_empty()) {
        message.context_id = None;
    }
    if message.task_id.as_ref().is_some_and(|t| t.0.is_empty()) {
        message.task_id = None;
    }
}
