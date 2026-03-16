// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Message types for the A2A protocol.
//!
//! A [`Message`] is the fundamental communication unit between a client and an
//! agent. Each message has a [`MessageRole`] (`ROLE_USER` or `ROLE_AGENT`) and
//! carries one or more [`Part`] values.
//!
//! # Part type discriminator
//!
//! [`Part`] uses a `type` field discriminator per the A2A spec:
//! - `{"type": "text", "text": "hi"}`
//! - `{"type": "file", "file": {"name": "f.png", "mimeType": "image/png", "bytes": "..."}}`
//! - `{"type": "data", "data": {...}}`

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
    #[inline]
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
        let s = match self {
            Self::Unspecified => "ROLE_UNSPECIFIED",
            Self::User => "ROLE_USER",
            Self::Agent => "ROLE_AGENT",
        };
        f.write_str(s)
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
/// Uses a `type` field discriminator per the A2A spec. In JSON:
/// - `{"type": "text", "text": "hello"}`
/// - `{"type": "file", "file": {"name": "f.png", "mimeType": "image/png", "bytes": "..."}}`
/// - `{"type": "data", "data": {...}}`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    /// The content of this part (text, file, or data).
    #[serde(flatten)]
    pub content: PartContent,

    /// Arbitrary metadata for this part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Part {
    /// Creates a text [`Part`] with the given content.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: PartContent::Text { text: text.into() },
            metadata: None,
        }
    }

    /// Creates a file [`Part`] from raw bytes (base64-encoded).
    #[must_use]
    pub fn file_bytes(bytes: impl Into<String>) -> Self {
        Self {
            content: PartContent::File {
                file: FileContent {
                    name: None,
                    mime_type: None,
                    bytes: Some(bytes.into()),
                    uri: None,
                },
            },
            metadata: None,
        }
    }

    /// Creates a file [`Part`] from a URI.
    #[must_use]
    pub fn file_uri(uri: impl Into<String>) -> Self {
        Self {
            content: PartContent::File {
                file: FileContent {
                    name: None,
                    mime_type: None,
                    bytes: None,
                    uri: Some(uri.into()),
                },
            },
            metadata: None,
        }
    }

    /// Creates a file [`Part`] with full metadata.
    #[must_use]
    pub fn file(file: FileContent) -> Self {
        Self {
            content: PartContent::File { file },
            metadata: None,
        }
    }

    /// Creates a data [`Part`] carrying structured JSON.
    #[must_use]
    pub const fn data(data: serde_json::Value) -> Self {
        Self {
            content: PartContent::Data { data },
            metadata: None,
        }
    }

    // ── Backward-compatible constructors ─────────────────────────────────

    /// Creates a raw (bytes) [`Part`] with base64-encoded data.
    ///
    /// **Deprecated:** Use [`Part::file_bytes`] instead. This constructor
    /// exists for backward compatibility during the v0.2→v0.3 migration.
    #[must_use]
    pub fn raw(raw: impl Into<String>) -> Self {
        Self::file_bytes(raw)
    }

    /// Creates a URL [`Part`].
    ///
    /// **Deprecated:** Use [`Part::file_uri`] instead. This constructor
    /// exists for backward compatibility during the v0.2→v0.3 migration.
    #[must_use]
    pub fn url(url: impl Into<String>) -> Self {
        Self::file_uri(url)
    }
}

// ── FileContent ──────────────────────────────────────────────────────────────

/// Content of a file part.
///
/// At least one of `bytes` or `uri` should be set. Both may be set if the
/// file is available via both inline data and a URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    /// Filename (e.g. `"report.pdf"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// MIME type (e.g. `"image/png"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Base64-encoded file content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<String>,

    /// URL to the file content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

impl FileContent {
    /// Creates a [`FileContent`] from inline base64 bytes.
    #[must_use]
    pub fn from_bytes(bytes: impl Into<String>) -> Self {
        Self {
            name: None,
            mime_type: None,
            bytes: Some(bytes.into()),
            uri: None,
        }
    }

    /// Creates a [`FileContent`] from a URI.
    #[must_use]
    pub fn from_uri(uri: impl Into<String>) -> Self {
        Self {
            name: None,
            mime_type: None,
            bytes: None,
            uri: Some(uri.into()),
        }
    }

    /// Sets the filename.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the MIME type.
    #[must_use]
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }
}

// ── PartContent ──────────────────────────────────────────────────────────────

/// The content of a [`Part`], discriminated by a `type` field per the A2A spec.
///
/// In JSON, the `type` field determines the variant:
/// - `"text"` → [`PartContent::Text`]
/// - `"file"` → [`PartContent::File`]
/// - `"data"` → [`PartContent::Data`]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PartContent {
    /// Plain-text content.
    #[serde(rename = "text")]
    Text {
        /// The text content.
        text: String,
    },
    /// File content (inline bytes and/or URI reference).
    #[serde(rename = "file")]
    File {
        /// The file content.
        file: FileContent,
    },
    /// Structured JSON data.
    #[serde(rename = "data")]
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
    fn text_part_has_type_discriminator() {
        let part = Part::text("hello world");
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(
            json.contains("\"type\":\"text\""),
            "should have type discriminator: {json}"
        );
        assert!(json.contains("\"text\":\"hello world\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back.content, PartContent::Text { .. }));
    }

    #[test]
    fn file_bytes_part_roundtrip() {
        let part = Part::file(
            FileContent::from_bytes("aGVsbG8=")
                .with_name("test.png")
                .with_mime_type("image/png"),
        );
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(
            json.contains("\"type\":\"file\""),
            "should have type discriminator: {json}"
        );
        assert!(json.contains("\"file\""));
        assert!(json.contains("\"name\":\"test.png\""));
        assert!(json.contains("\"mimeType\":\"image/png\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        match back.content {
            PartContent::File { file } => {
                assert_eq!(file.name.as_deref(), Some("test.png"));
                assert_eq!(file.mime_type.as_deref(), Some("image/png"));
                assert_eq!(file.bytes.as_deref(), Some("aGVsbG8="));
            }
            _ => panic!("expected File variant"),
        }
    }

    #[test]
    fn file_uri_part_roundtrip() {
        let part = Part::file_uri("https://example.com/file.pdf");
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(json.contains("\"type\":\"file\""));
        assert!(json.contains("\"uri\":\"https://example.com/file.pdf\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        match back.content {
            PartContent::File { file } => {
                assert_eq!(file.uri.as_deref(), Some("https://example.com/file.pdf"));
            }
            _ => panic!("expected File variant"),
        }
    }

    #[test]
    fn data_part_has_type_discriminator() {
        let part = Part::data(serde_json::json!({"key": "value"}));
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(
            json.contains("\"type\":\"data\""),
            "should have type discriminator: {json}"
        );
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
                Part::file_bytes("aGVsbG8="),
                Part::file_uri("https://example.com/output.pdf"),
            ],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        };

        let json = serde_json::to_string(&msg).expect("serialize mixed-part message");
        assert!(json.contains("\"text\":\"Here is the result\""));
        assert!(json.contains("\"type\":\"file\""));

        let back: Message = serde_json::from_str(&json).expect("deserialize mixed-part message");
        assert_eq!(back.parts.len(), 3);
        assert!(
            matches!(&back.parts[0].content, PartContent::Text { text } if text == "Here is the result")
        );
        assert!(matches!(&back.parts[1].content, PartContent::File { .. }));
        assert!(matches!(&back.parts[2].content, PartContent::File { .. }));
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

    #[test]
    fn backward_compat_raw_constructor() {
        let part = Part::raw("aGVsbG8=");
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(json.contains("\"type\":\"file\""));
        assert!(json.contains("\"bytes\":\"aGVsbG8=\""));
    }

    #[test]
    fn backward_compat_url_constructor() {
        let part = Part::url("https://example.com/file.pdf");
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(json.contains("\"type\":\"file\""));
        assert!(json.contains("\"uri\":\"https://example.com/file.pdf\""));
    }
}
