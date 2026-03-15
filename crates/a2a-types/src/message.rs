// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Message types for the A2A protocol.
//!
//! A [`Message`] is the fundamental communication unit between a client and an
//! agent. Each message has a [`MessageRole`] (`user` or `agent`) and carries
//! one or more [`Part`] values (text, file, or structured data).
//!
//! # Part discriminated union
//!
//! [`Part`] uses `#[serde(tag = "kind")]` so that `{"kind":"text","text":"hi"}`
//! deserializes to `Part::Text(TextPart { text: "hi", .. })`.
//!
//! # File content
//!
//! [`FileContent`] is an untagged union: if the JSON object has a `bytes` field
//! it deserializes as [`FileWithBytes`]; if it has a `uri` field it deserializes
//! as [`FileWithUri`].

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// Sent by the human/client side.
    User,
    /// Sent by the agent.
    Agent,
}

// ── Message ───────────────────────────────────────────────────────────────────

/// A message exchanged between a client and an agent.
///
/// The wire `kind` field (`"message"`) is injected by enclosing discriminated
/// unions such as [`crate::events::StreamResponse`] and
/// [`crate::responses::SendMessageResponse`]. Standalone `Message` values
/// received over the wire may include `kind`; serde silently tolerates unknown
/// fields, so no action is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Unique message identifier.
    #[serde(rename = "messageId")]
    pub id: MessageId,

    /// Role of the message originator.
    pub role: MessageRole,

    /// Message content parts (must contain at least one element).
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
/// Discriminated by the `"kind"` field: `"text"`, `"file"`, or `"data"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Part {
    /// Plain-text content.
    Text(TextPart),
    /// File content (bytes or URI reference).
    File(FilePart),
    /// Structured JSON data.
    Data(DataPart),
}

// ── TextPart ──────────────────────────────────────────────────────────────────

/// A plain-text message part.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPart {
    /// The text content.
    pub text: String,

    /// Arbitrary metadata for this part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl TextPart {
    /// Creates a [`TextPart`] with the given text and no metadata.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            metadata: None,
        }
    }
}

// ── FilePart ──────────────────────────────────────────────────────────────────

/// A file message part, carrying either inline bytes or a URI reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePart {
    /// The file content (inline bytes or URI).
    pub file: FileContent,

    /// Arbitrary metadata for this part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ── FileContent ───────────────────────────────────────────────────────────────

/// File content: either inline base64-encoded bytes or a URI reference.
///
/// This is an untagged union. Serde tries [`FileWithBytes`] first (matches when
/// a `bytes` field is present), then [`FileWithUri`] (matches when a `uri`
/// field is present).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileContent {
    /// File data encoded as a base64 string in the `bytes` field.
    Bytes(FileWithBytes),
    /// File accessible via a URI.
    Uri(FileWithUri),
}

// ── FileWithBytes ─────────────────────────────────────────────────────────────

/// Inline file content: base64-encoded bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWithBytes {
    /// Optional file name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// MIME type of the file (e.g. `"image/png"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Base64-encoded file content.
    pub bytes: String,
}

// ── FileWithUri ───────────────────────────────────────────────────────────────

/// File content accessible via a URI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWithUri {
    /// Optional file name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// MIME type of the file (e.g. `"application/pdf"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Absolute URI of the file.
    pub uri: String,
}

// ── DataPart ──────────────────────────────────────────────────────────────────

/// A structured-data message part carrying an arbitrary JSON object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataPart {
    /// Structured JSON payload (should be an object per spec).
    pub data: serde_json::Value,

    /// Arbitrary metadata for this part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message() -> Message {
        Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::Text(TextPart::new("Hello"))],
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
        assert!(json.contains("\"role\":\"user\""));

        let back: Message = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, MessageId::new("msg-1"));
        assert_eq!(back.role, MessageRole::User);
    }

    #[test]
    fn text_part_roundtrip() {
        let part = Part::Text(TextPart::new("hello world"));
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(json.contains("\"kind\":\"text\""));
        assert!(json.contains("\"text\":\"hello world\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, Part::Text(_)));
    }

    #[test]
    fn file_part_bytes_roundtrip() {
        let part = Part::File(FilePart {
            file: FileContent::Bytes(FileWithBytes {
                name: Some("test.png".into()),
                mime_type: Some("image/png".into()),
                bytes: "aGVsbG8=".into(),
            }),
            metadata: None,
        });
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(json.contains("\"bytes\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, Part::File(_)));
    }

    #[test]
    fn file_part_uri_roundtrip() {
        let part = Part::File(FilePart {
            file: FileContent::Uri(FileWithUri {
                name: None,
                mime_type: None,
                uri: "https://example.com/file.pdf".into(),
            }),
            metadata: None,
        });
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(json.contains("\"uri\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, Part::File(_)));
    }

    #[test]
    fn data_part_roundtrip() {
        let part = Part::Data(DataPart {
            data: serde_json::json!({"key": "value"}),
            metadata: None,
        });
        let json = serde_json::to_string(&part).expect("serialize");
        assert!(json.contains("\"kind\":\"data\""));
        let back: Part = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, Part::Data(_)));
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
}
