// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Agent extension and card-signature types.
//!
//! Extensions allow agents to advertise optional capabilities beyond the core
//! A2A 0.3.0 specification. [`AgentExtension`] is referenced by
//! [`crate::agent_card::AgentCapabilities`].

use serde::{Deserialize, Serialize};

// ── AgentExtension ────────────────────────────────────────────────────────────

/// Describes an optional extension that an agent supports.
///
/// Extensions are identified by a URI and may carry an arbitrary JSON
/// parameter block understood by the extension spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExtension {
    /// Unique URI identifying the extension (e.g. `"https://example.com/ext/v1"`).
    pub uri: String,

    /// Human-readable description of the extension's purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether clients **must** support this extension to interact correctly.
    ///
    /// A value of `true` means the agent cannot operate meaningfully without
    /// the client understanding this extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// Extension-specific parameters; structure is defined by the extension URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl AgentExtension {
    /// Creates a minimal [`AgentExtension`] with only a URI.
    #[must_use]
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            description: None,
            required: None,
            params: None,
        }
    }
}

// ── AgentCardSignature ────────────────────────────────────────────────────────

/// A cryptographic signature over an [`crate::agent_card::AgentCard`].
///
/// The exact schema for this type is an extension point in A2A 0.3.0 and is
/// expected to be defined by a future signature specification. The raw
/// [`serde_json::Value`] representation is used until the schema is finalised.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCardSignature(
    /// Raw JSON object carrying the signature fields.
    pub serde_json::Value,
);

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_extension_minimal_roundtrip() {
        let ext = AgentExtension::new("https://example.com/ext/v1");
        let json = serde_json::to_string(&ext).expect("serialize");
        assert!(json.contains("\"uri\""));
        assert!(
            !json.contains("\"description\""),
            "None fields must be omitted"
        );

        let back: AgentExtension = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.uri, "https://example.com/ext/v1");
    }

    #[test]
    fn agent_extension_full_roundtrip() {
        let mut ext = AgentExtension::new("https://example.com/ext/v1");
        ext.description = Some("Cool extension".into());
        ext.required = Some(true);
        ext.params = Some(serde_json::json!({"version": 2}));

        let json = serde_json::to_string(&ext).expect("serialize");
        let back: AgentExtension = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.description.as_deref(), Some("Cool extension"));
        assert_eq!(back.required, Some(true));
    }
}
