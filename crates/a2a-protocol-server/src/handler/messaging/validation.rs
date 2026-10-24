// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Ingress validation of a `SendMessage` request: ids, parts and metadata.
//!
//! Runs before anything with a side effect, so a rejected request leaves no
//! task row, queue or token behind. The checks are in the order the errors
//! were always reported in, which matters to a client that fixes one problem
//! at a time.

use a2a_protocol_types::message::PartContent;
use a2a_protocol_types::params::MessageSendParams;

use super::super::RequestHandler;
use super::super::helpers::{unset_if_empty, validate_id, validate_metadata_object};
use super::decisions::json_byte_len;
use crate::error::{ServerError, ServerResult};
use crate::portable::{inexact_number, standard_base64};

/// Which file references a message may carry; see
/// [`refuse_private_file_urls`](crate::RequestHandlerBuilder::refuse_private_file_urls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileUrls {
    /// Any, unchecked: the default.
    Any,
    /// Only those [`check_public_url`](crate::url_guard::check_public_url)
    /// accepts.
    PublicOnly,
}

impl RequestHandler {
    /// Validates the request in place: proto3 defaults are unset, then ids,
    /// parts and every `metadata` field are checked against the handler's
    /// limits.
    ///
    /// # Errors
    ///
    /// [`ServerError::InvalidParams`] naming the first field that failed.
    /// Spec §14: "File references within A2A messages MUST be validated to
    /// prevent SSRF", when the builder asked for it
    /// (`refuse_private_file_urls`).
    fn check_file_urls(&self, message: &a2a_protocol_types::Message) -> ServerResult<()> {
        if self.file_urls == FileUrls::Any {
            return Ok(());
        }
        for (i, part) in message.parts.iter().enumerate() {
            if let a2a_protocol_types::message::PartContent::Url(url) = &part.content {
                crate::url_guard::check(url, "file URL").map_err(|e| {
                    ServerError::InvalidField {
                        field: format!("message.parts[{i}].url"),
                        message: e.message,
                    }
                })?;
            }
        }
        Ok(())
    }

    pub(super) fn validate_send_params(
        &self,
        params: &mut MessageSendParams,
    ) -> ServerResult<crate::request_context::ContentCheck> {
        // The signature covers the message as it was sent, so it is checked
        // before anything below rewrites it (empty strings unset, base64
        // re-padded); ADR 0027.
        let content_check = self.inbound.check(&params.message)?;
        unset_proto3_defaults(params);
        self.validate_send_ids(params)?;

        // a2a.proto marks `Message.role` REQUIRED. The JSON bindings already
        // refuse a message without one, but over gRPC an absent enum decodes
        // as 0, `ROLE_UNSPECIFIED`, and was accepted until 2026-10-10, so the
        // bindings disagreed about the same request.
        if params.message.role == a2a_protocol_types::message::MessageRole::Unspecified {
            return Err(ServerError::InvalidField {
                field: "message.role".into(),
                message: "message.role is required: ROLE_USER or ROLE_AGENT".into(),
            });
        }

        // SC-4: Reject messages with no parts.
        if params.message.parts.is_empty() {
            return Err(ServerError::InvalidField {
                field: "message.parts".into(),
                message: "message must contain at least one part".into(),
            });
        }

        // Spec §3.1.1: a part whose media type the card does not declare is
        // ContentTypeNotSupportedError (ACTS CORE-SEND-004).
        self.ensure_input_modes_supported(&params.message)?;
        self.check_file_urls(&params.message)?;

        // Cross-binding portability: every client-supplied `metadata` field
        // must be a JSON object so the resulting task is representable over
        // gRPC (google.protobuf.Struct), not just over JSON-RPC/REST. Reject
        // arrays and scalars at ingress rather than storing a task that one
        // binding can serve and another cannot.
        validate_metadata_object(
            params.message.metadata.as_ref(),
            "message",
            "message.metadata",
        )?;
        validate_metadata_object(params.metadata.as_ref(), "request", "metadata")?;
        for (i, part) in params.message.parts.iter().enumerate() {
            validate_metadata_object(
                part.metadata.as_ref(),
                &format!("message part {i}"),
                &format!("message.parts[{i}].metadata"),
            )?;
        }
        make_portable(params)?;

        // PR-8: Reject oversized metadata to prevent memory exhaustion.
        let max_meta = self.limits.max_metadata_size;
        check_metadata_size(
            params.message.metadata.as_ref(),
            "message",
            "message.metadata",
            max_meta,
        )?;
        check_metadata_size(params.metadata.as_ref(), "request", "metadata", max_meta)?;
        Ok(content_check)
    }

    /// The ids of a `SendMessage`: the message's, each checked by
    /// `validate_id`, and an inline push config's.
    fn validate_send_ids(&self, params: &MessageSendParams) -> ServerResult<()> {
        // Validate incoming IDs: reject empty/whitespace-only values, control
        // characters and excessively long values (AP-1; spec §14).
        //
        // Each is named by its path on the wire, as a key missing from the
        // JSON is (`message.messageId`) and as every other operation names
        // its ids (`taskId`, `contextId`). Until 2026-10-23 these four used
        // the Rust names (`message.id`, `task_id`, `context_id`,
        // `reference_task_ids`), so the same field had two names in the
        // `BadRequest` detail depending on whether it was absent or empty.
        //
        // `message.id` is checked with the same rule as the other two. It was
        // not, and it is the one that reaches furthest: it is stored verbatim
        // in `idempotency_keys.message_id`, it keys the history de-duplication
        // in `helpers.rs`, and it is interpolated into the error a caller sees
        // when an idempotency key is already held by a different message. The
        // key beside it in that same row is capped and charset-restricted
        // precisely because "a key reaches store keys and log lines, and a
        // control character or newline in either is how log injection starts";
        // the message id reached both with neither defence, bounded only by
        // the dispatcher's request-body limit.
        //
        // Bounded by `effective_max_message_id_length`, not `max_id_length`:
        // a message id is conventionally a UUID, so a deployment that
        // tightened `max_id_length` below 36 refused every conformant client.
        // See that method for the whole account.
        validate_id(
            &params.message.id.0,
            "message.messageId",
            self.limits.effective_max_message_id_length(),
        )?;
        if let Some(ref ctx_id) = params.message.context_id {
            validate_id(&ctx_id.0, "message.contextId", self.limits.max_id_length)?;
        }
        if let Some(ref task_id) = params.message.task_id {
            validate_id(&task_id.0, "message.taskId", self.limits.max_id_length)?;
        }
        // Task ids like `task_id`, so the same checks (spec §13.4); they were
        // stored unchecked.
        for reference in params.message.reference_task_ids.iter().flatten() {
            validate_id(
                &reference.0,
                "message.referenceTaskIds",
                self.limits.max_id_length,
            )?;
        }
        // An inline push config's ids are checked again when it is stored,
        // which is after the task is created: refused here, nothing is.
        if let Some(config) = params
            .configuration
            .as_ref()
            .and_then(|c| c.task_push_notification_config.as_ref())
        {
            super::super::ids::refuse_control_chars_in_push_config(config)?;
        }
        Ok(())
    }
}

/// The rest of cross-binding portability (spec §4: every binding MUST
/// represent the same thing). A part's `raw` is protobuf `bytes`: it must be
/// base64, read as the gRPC conversion reads it (standard or URL-safe,
/// padded or not), and is kept in the standard padded form a gRPC read
/// gives. The numbers in metadata and in a `data` part are protobuf
/// doubles, which the gRPC conversion refuses beyond ±2^53 rather than
/// round. Until 2026-10-13 any `raw` string and any integer were stored,
/// and the task holding them could not be read over gRPC: `GetTask`, every
/// `ListTasks` page with it and `SubscribeToTask` answered `INTERNAL`.
fn make_portable(params: &mut MessageSendParams) -> ServerResult<()> {
    let inexact = |value: Option<&serde_json::Value>, field: &dyn Fn() -> String| {
        value.and_then(inexact_number).map_or(Ok(()), |n| {
            let field = field();
            Err(ServerError::InvalidField {
                message: format!(
                    "{field} holds the integer {n}, beyond the ±2^53 a protobuf number holds exactly"
                ),
                field,
            })
        })
    };
    inexact(params.metadata.as_ref(), &|| "metadata".to_owned())?;
    inexact(params.message.metadata.as_ref(), &|| {
        "message.metadata".to_owned()
    })?;
    for (i, part) in params.message.parts.iter_mut().enumerate() {
        inexact(part.metadata.as_ref(), &|| {
            format!("message.parts[{i}].metadata")
        })?;
        match &mut part.content {
            PartContent::Raw(raw) => {
                *raw = standard_base64(raw).map_err(|e| ServerError::InvalidField {
                    field: format!("message.parts[{i}].raw"),
                    message: format!("message.parts[{i}].raw is not base64: {e}"),
                })?;
            }
            PartContent::Data(data) => inexact(Some(data), &|| format!("message.parts[{i}].data"))?,
            _ => {}
        }
    }
    Ok(())
}

/// Rejects a `metadata` object whose JSON encoding exceeds `max_meta` bytes.
///
/// Uses a byte-counting writer rather than serializing to a throwaway
/// `String`, so measuring the limit costs no allocation. `what` names the
/// field in the error — `"message"` or `"request"` — and `path` is its path
/// in the request, for the error's `BadRequest` detail.
fn check_metadata_size(
    metadata: Option<&serde_json::Value>,
    what: &str,
    path: &str,
    max_meta: usize,
) -> ServerResult<()> {
    let Some(meta) = metadata else {
        return Ok(());
    };
    let meta_size = json_byte_len(meta)
        .map_err(|_| ServerError::InvalidParams(format!("{what} metadata is not serializable")))?;
    if meta_size > max_meta {
        return Err(ServerError::InvalidField {
            field: path.to_owned(),
            message: format!(
                "{what} metadata exceeds maximum size ({meta_size} bytes, max {max_meta})"
            ),
        });
    }
    Ok(())
}

/// Maps the exactly-empty proto3 strings of an incoming send to absent: the
/// message's `contextId` and `taskId`, each part's `mediaType` and
/// `filename`, and the inline push config's `id` and `token`.
///
/// The A2A JSON bindings are `ProtoJSON`, and these are proto3 strings
/// without presence: an empty string *is* the unset value, as the gRPC
/// conversion reads it. A client that prints every field — a2a-java's
/// JSON-RPC transport uses `alwaysPrintFieldsWithNoPresence` — therefore
/// sends `"contextId": ""` for "none", and until 2026-09-09 this server
/// rejected that as an invalid id, failing every JSON-RPC pairing with the
/// Java SDK in the official ITK while the same peer passed over gRPC and
/// HTTP+JSON, whose printers omit defaults. Until 2026-10-13 the rest were
/// read as set: a part's `"mediaType": ""` was checked against the card's
/// input modes and refused. Whitespace-only is left alone and still rejected
/// by `validate_id`: no printer produces it for "unset".
fn unset_proto3_defaults(params: &mut MessageSendParams) {
    let message = &mut params.message;
    if message.context_id.as_ref().is_some_and(|c| c.0.is_empty()) {
        message.context_id = None;
    }
    if message.task_id.as_ref().is_some_and(|t| t.0.is_empty()) {
        message.task_id = None;
    }
    for part in &mut message.parts {
        unset_if_empty(&mut part.media_type);
        unset_if_empty(&mut part.filename);
    }
    if let Some(config) = params
        .configuration
        .as_mut()
        .and_then(|c| c.task_push_notification_config.as_mut())
    {
        super::super::push_config::unset_proto3_defaults(config);
    }
}
