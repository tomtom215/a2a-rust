// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Message types for the A2A protocol.
//!
//! A [`Message`] is the fundamental communication unit between a client and an
//! agent. Each message has a [`MessageRole`] (`ROLE_USER` or `ROLE_AGENT`) and
//! carries one or more [`Part`] values.
//!
//! # Part oneof
//!
//! [`Part`] is a flat struct with a [`PartContent`] oneof discriminated by
//! field presence: `{"text": "hi"}`, `{"raw": "base64..."}`, `{"url": "..."}`,
//! or `{"data": {...}}`.

use serde::{Deserialize, Serialize};

use crate::task::{ContextId, TaskId};

// ── MessageId ─────────────────────────────────────────────────────────────────

/// Opaque unique identifier for a [`Message`].
///
/// Wraps a `String` for compile-time type safety — a [`MessageId`] cannot be
/// accidentally passed where a [`TaskId`] is expected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub String);

impl MessageId {
    /// Creates a new [`MessageId`] from any string-like value.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for MessageId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MessageId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for MessageId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ── MessageRole ───────────────────────────────────────────────────────────────

/// The originator of a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageRole {
    /// Proto default (0-value); should not appear in normal usage.
    #[serde(rename = "ROLE_UNSPECIFIED")]
    Unspecified,
    /// Sent by the human/client side.
    #[serde(rename = "ROLE_USER")]
    User,
    /// Sent by the agent.
    #[serde(rename = "ROLE_AGENT")]
    Agent,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_string(self).unwrap_or_else(|_| "ROLE_UNSPECIFIED".into());
        f.write_str(s.trim_matches('"'))
    }
}

// ── Message ───────────────────────────────────────────────────────────────────

/// A message exchanged between a client and an agent.
///
/// The wire `kind` field (`"message"`) is injected by enclosing discriminated
/// unions such as [`crate::events::StreamResponse`] and
/// [`crate::responses::SendMessageResponse`]. Standalone `Message` values
/// received over the wire may include `kind`; serde silently tolerates unknown
/// fields, so no action is needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Unique message identifier.
    #[serde(rename = "messageId")]
    pub id: MessageId,

    /// Role of the message originator.
    pub role: MessageRole,

    /// Message content parts.
    ///
    /// **Spec requirement:** Must contain at least one element. The A2A
    /// protocol does not define behavior for empty parts lists.
    pub parts: Vec<Part>,

    /// Task this message belongs to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,

    /// Conversation context this message belongs to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<ContextId>,

    /// IDs of tasks referenced by this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_task_ids: Option<Vec<TaskId>>,

    /// URIs of extensions used in this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,

    /// Arbitrary metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ── Part ─────────────────────────────────────────────────────────────────────

/// A content part within a [`Message`] or [`crate::artifact::Artifact`].
///
/// A flat struct with a [`PartContent`] oneof and common fields. In JSON,
/// exactly one of `text`, `raw`, `url`, or `data` is present, which
/// determines the content type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    /// The content of this part (one of text, raw, url, or data).
    #[serde(flatten)]
    pub content: PartContent,

    /// Arbitrary metadata for this part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    /// Optional filename associated with this part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// MIME type of the content (e.g. `"image/png"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

impl Part {
    /// Creates a text [`Part`] with the given content.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: PartContent::Text { text: text.into() },
            metadata: None,
            filename: None,
            media_type: None,
        }
    }

    /// Creates a raw (bytes) [`Part`] with base64-encoded data.
    #[must_use]
    pub fn raw(raw: impl Into<String>) -> Self {
        Self {
            content: PartContent::Raw { raw: raw.into() },
            metadata: None,
            filename: None,
            media_type: None,
        }
    }

    /// Creates a URL [`Part`].
    #[must_use]
    pub fn url(url: impl Into<String>) -> Self {
        Self {
            content: PartContent::Url { url: url.into() },
            metadata: None,
            filename: None,
            media_type: None,
        }
    }

    /// Creates a data [`Part`] carrying structured JSON.
    #[must_use]
    pub const fn data(data: serde_json::Value) -> Self {
        Self {
            content: PartContent::Data { data },
            metadata: None,
            filename: None,
            media_type: None,
        }
    }
}

// ── PartContent ──────────────────────────────────────────────────────────────

/// The content of a [`Part`], discriminated by field presence (proto oneof).
///
/// In JSON, exactly one field is present: `"text"`, `"raw"`, `"url"`, or
/// `"data"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PartContent {
    /// Plain-text content.
    Text {
        /// The text content.
        text: String,
    },
    /// Raw binary content (base64-encoded in JSON).
    Raw {
        /// Base64-encoded bytes.
        raw: String,
    },
    /// URL reference to content.
    Url {
        /// Absolute URL.
        url: String,
    },
    /// Structured JSON data.
    Data {
        /// Structured JSON payload.
        data: serde_json::Value,
    },
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message() -> Message {
        Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::text("Hello")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        }
    }

    #[test]
    fn message_roundtrip() {
        let msg = make_message();
        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains("\"messageId\":\"msg-1\""));
        assert!(json.contains("\"role\":\"ROLE_USER\""));

        let back: Message = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, MessageId::new("msg-1"));
        assert_eq!(back.role, MessageRole::User);
    }

    #[test]
    fn text_part_roundtrip() {
        let part = Part::text("hello world");
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(!json.contains("\"kind\""), "v1.0 should not have kind tag");
        assert!(json.contains("\"text\":\"hello world\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back.content, PartContent::Text { .. }));
    }

    #[test]
    fn raw_part_roundtrip() {
        let mut part = Part::raw("aGVsbG8=");
        part.filename = Some("test.png".into());
        part.media_type = Some("image/png".into());
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(json.contains("\"raw\""));
        assert!(json.contains("\"filename\""));
        assert!(json.contains("\"mediaType\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back.content, PartContent::Raw { .. }));
        assert_eq!(back.filename.as_deref(), Some("test.png"));
    }

    #[test]
    fn url_part_roundtrip() {
        let part = Part::url("https://example.com/file.pdf");
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(json.contains("\"url\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back.content, PartContent::Url { .. }));
    }

    #[test]
    fn data_part_roundtrip() {
        let part = Part::data(serde_json::json!({"key": "value"}));
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(!json.contains("\"kind\""), "v1.0 should not have kind tag");
        assert!(json.contains("\"data\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back.content, PartContent::Data { .. }));
    }

    #[test]
    fn none_fields_omitted() {
        let msg = make_message();
        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(
            !json.contains("\"taskId\""),
            "taskId should be omitted: {json}"
        );
        assert!(
            !json.contains("\"metadata\""),
            "metadata should be omitted: {json}"
        );
    }

    #[test]
    fn wire_format_role_unspecified_roundtrip() {
        let json = serde_json::to_string(&MessageRole::Unspecified).unwrap();
        assert_eq!(json, "\"ROLE_UNSPECIFIED\"");

        let back: MessageRole = serde_json::from_str("\"ROLE_UNSPECIFIED\"").unwrap();
        assert_eq!(back, MessageRole::Unspecified);
    }

    #[test]
    fn message_role_display_trait() {
        assert_eq!(MessageRole::User.to_string(), "ROLE_USER");
        assert_eq!(MessageRole::Agent.to_string(), "ROLE_AGENT");
        assert_eq!(MessageRole::Unspecified.to_string(), "ROLE_UNSPECIFIED");
    }

    #[test]
    fn mixed_part_message_roundtrip() {
        let msg = Message {
            id: MessageId::new("msg-mixed"),
            role: MessageRole::Agent,
            parts: vec![
                Part::text("Here is the result"),
                Part::raw("aGVsbG8="),
                Part::url("https://example.com/output.pdf"),
            ],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        };

        let json = serde_json::to_string(&msg).expect("serialize mixed-part message");
        assert!(json.contains("\"text\":\"Here is the result\""));
        assert!(json.contains("\"raw\":\"aGVsbG8=\""));
        assert!(json.contains("\"url\":\"https://example.com/output.pdf\""));

        let back: Message = serde_json::from_str(&json).expect("deserialize mixed-part message");
        assert_eq!(back.parts.len(), 3);
        assert!(
            matches!(&back.parts[0].content, PartContent::Text { text } if text == "Here is the result")
        );
        assert!(matches!(&back.parts[1].content, PartContent::Raw { raw } if raw == "aGVsbG8="));
        assert!(
            matches!(&back.parts[2].content, PartContent::Url { url } if url == "https://example.com/output.pdf")
        );
    }

    #[test]
    fn message_with_reference_task_ids() {
        use crate::task::TaskId;

        let msg = Message {
            id: MessageId::new("msg-ref"),
            role: MessageRole::User,
            parts: vec![Part::text("check these tasks")],
            task_id: None,
            context_id: None,
            reference_task_ids: Some(vec![TaskId::new("task-100"), TaskId::new("task-200")]),
            extensions: None,
            metadata: None,
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(
            json.contains("\"referenceTaskIds\""),
            "referenceTaskIds should be present: {json}"
        );
        assert!(json.contains("\"task-100\""));
        assert!(json.contains("\"task-200\""));

        let back: Message = serde_json::from_str(&json).expect("deserialize");
        let refs = back
            .reference_task_ids
            .expect("should have reference_task_ids");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0], TaskId::new("task-100"));
        assert_eq!(refs[1], TaskId::new("task-200"));
    }
}
