// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Artifact types for the A2A protocol.
//!
//! An [`Artifact`] represents a discrete output produced by an agent — for
//! example a generated file, a code snippet, or a structured result. Artifacts
//! are carried in [`crate::task::Task::artifacts`] and in
//! [`crate::events::TaskArtifactUpdateEvent`].

use serde::{Deserialize, Serialize};

use crate::message::Part;

// ── ArtifactId ────────────────────────────────────────────────────────────────

/// Opaque unique identifier for an [`Artifact`].
///
/// Wraps a `String` for compile-time type safety.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactId(pub String);

impl ArtifactId {
    /// Creates a new [`ArtifactId`] from any string-like value.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ArtifactId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ArtifactId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for ArtifactId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ── Artifact ──────────────────────────────────────────────────────────────────

/// An output artifact produced by an agent.
///
/// Each artifact has a unique [`ArtifactId`] and carries its content as a
/// non-empty list of [`Part`] values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    /// Unique artifact identifier.
    #[serde(rename = "artifactId")]
    pub id: ArtifactId,

    /// Optional human-readable name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Content parts (must contain at least one element).
    pub parts: Vec<Part>,

    /// URIs of extensions used in this artifact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,

    /// Arbitrary metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Artifact {
    /// Creates a minimal [`Artifact`] with an ID and a single part.
    #[must_use]
    pub fn new(id: impl Into<ArtifactId>, parts: Vec<Part>) -> Self {
        Self {
            id: id.into(),
            name: None,
            description: None,
            parts,
            extensions: None,
            metadata: None,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Part, TextPart};

    #[test]
    fn artifact_roundtrip() {
        let artifact = Artifact::new("art-1", vec![Part::Text(TextPart::new("result content"))]);
        let json = serde_json::to_string(&artifact).expect("serialize");
        assert!(json.contains("\"artifactId\":\"art-1\""));
        assert!(json.contains("\"kind\":\"text\""));

        let back: Artifact = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, ArtifactId::new("art-1"));
        assert_eq!(back.parts.len(), 1);
    }

    #[test]
    fn optional_fields_omitted() {
        let artifact = Artifact::new("art-2", vec![Part::Text(TextPart::new("x"))]);
        let json = serde_json::to_string(&artifact).expect("serialize");
        assert!(!json.contains("\"name\""), "name should be omitted");
        assert!(
            !json.contains("\"description\""),
            "description should be omitted"
        );
        assert!(!json.contains("\"metadata\""), "metadata should be omitted");
    }

    #[test]
    fn artifact_id_display() {
        let id = ArtifactId::new("my-artifact");
        assert_eq!(id.to_string(), "my-artifact");
    }
}
