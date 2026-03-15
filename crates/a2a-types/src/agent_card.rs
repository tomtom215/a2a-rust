// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Agent card and capability discovery types.
//!
//! The [`AgentCard`] is the root discovery document served by an A2A agent at
//! `/.well-known/agent-card.json`. It describes the agent's identity,
//! capabilities, skills, security requirements, and transport preferences.
//!
//! # Transport protocol
//!
//! [`TransportProtocol`] uses `SCREAMING_SNAKE_CASE`-adjacent serialization.
//! `JSONRPC` and `GRPC` do not contain underscores in their wire representations,
//! so explicit `#[serde(rename)]` attributes are used where the derive would
//! produce a different result.

use serde::{Deserialize, Serialize};

use crate::extensions::{AgentCardSignature, AgentExtension};
use crate::security::{NamedSecuritySchemes, SecurityRequirements};

// ── TransportProtocol ─────────────────────────────────────────────────────────

/// The transport protocol used by an agent interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransportProtocol {
    /// JSON-RPC 2.0 over HTTP(S).
    #[serde(rename = "JSONRPC")]
    JsonRpc,

    /// gRPC.
    #[serde(rename = "GRPC")]
    Grpc,

    /// HTTP REST.
    #[serde(rename = "REST")]
    Rest,
}

// ── AgentInterface ────────────────────────────────────────────────────────────

/// An alternative transport interface offered by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInterface {
    /// Base URL of this interface endpoint.
    pub url: String,

    /// Transport protocol for this interface.
    pub transport: TransportProtocol,
}

// ── AgentCapabilities ─────────────────────────────────────────────────────────

/// Optional capability flags advertised by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    /// Whether the agent supports streaming via `message/stream`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,

    /// Whether the agent supports push notification delivery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_notifications: Option<bool>,

    /// Whether the agent retains full state-transition history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_transition_history: Option<bool>,

    /// Optional extensions supported by this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<AgentExtension>>,
}

impl AgentCapabilities {
    /// Creates an [`AgentCapabilities`] with all flags unset.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            streaming: None,
            push_notifications: None,
            state_transition_history: None,
            extensions: None,
        }
    }
}

// ── AgentProvider ─────────────────────────────────────────────────────────────

/// The organization that operates or publishes the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvider {
    /// Name of the organization.
    pub organization: String,

    /// URL of the organization's website.
    pub url: String,
}

// ── AgentSkill ────────────────────────────────────────────────────────────────

/// A discrete capability offered by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    /// Unique skill identifier within the agent.
    pub id: String,

    /// Human-readable skill name.
    pub name: String,

    /// Human-readable description of what the skill does.
    pub description: String,

    /// Searchable tags for the skill.
    pub tags: Vec<String>,

    /// Example prompts illustrating how to invoke the skill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<String>>,

    /// MIME types accepted as input by this skill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_modes: Option<Vec<String>>,

    /// MIME types produced as output by this skill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_modes: Option<Vec<String>>,

    /// Security requirements specific to this skill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<SecurityRequirements>,
}

// ── AgentCard ─────────────────────────────────────────────────────────────────

/// The root discovery document for an A2A agent.
///
/// Served at `/.well-known/agent-card.json`. Clients fetch this document to
/// discover the agent's endpoint URL, capabilities, skills, and security
/// requirements before establishing a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    /// A2A protocol version string (e.g. `"0.3.0"`).
    pub protocol_version: String,

    /// Display name of the agent.
    pub name: String,

    /// Human-readable description of the agent's purpose.
    pub description: String,

    /// Semantic version of this agent implementation.
    pub version: String,

    /// Base URL of the agent's primary RPC endpoint.
    pub url: String,

    /// Preferred transport protocol.
    pub preferred_transport: TransportProtocol,

    /// Additional transport interfaces offered by this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_interfaces: Option<Vec<AgentInterface>>,

    /// Default MIME types accepted as input.
    pub default_input_modes: Vec<String>,

    /// Default MIME types produced as output.
    pub default_output_modes: Vec<String>,

    /// Skills offered by this agent.
    pub skills: Vec<AgentSkill>,

    /// Capability flags.
    pub capabilities: AgentCapabilities,

    /// The organization operating this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<AgentProvider>,

    /// URL of the agent's icon image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,

    /// URL of the agent's documentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,

    /// Named security scheme definitions (OpenAPI-style).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_schemes: Option<NamedSecuritySchemes>,

    /// Global security requirements for the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<SecurityRequirements>,

    /// Whether this agent serves an authenticated extended card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_authenticated_extended_card: Option<bool>,

    /// Cryptographic signatures over this card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signatures: Option<Vec<AgentCardSignature>>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_card() -> AgentCard {
        AgentCard {
            protocol_version: "0.3.0".into(),
            name: "Test Agent".into(),
            description: "A test agent".into(),
            version: "1.0.0".into(),
            url: "https://agent.example.com/rpc".into(),
            preferred_transport: TransportProtocol::JsonRpc,
            additional_interfaces: None,
            default_input_modes: vec!["text/plain".into()],
            default_output_modes: vec!["text/plain".into()],
            skills: vec![AgentSkill {
                id: "echo".into(),
                name: "Echo".into(),
                description: "Echoes input".into(),
                tags: vec!["echo".into()],
                examples: None,
                input_modes: None,
                output_modes: None,
                security: None,
            }],
            capabilities: AgentCapabilities::none(),
            provider: None,
            icon_url: None,
            documentation_url: None,
            security_schemes: None,
            security: None,
            supports_authenticated_extended_card: None,
            signatures: None,
        }
    }

    #[test]
    fn transport_protocol_serialization() {
        assert_eq!(
            serde_json::to_string(&TransportProtocol::JsonRpc).expect("ser"),
            "\"JSONRPC\""
        );
        assert_eq!(
            serde_json::to_string(&TransportProtocol::Grpc).expect("ser"),
            "\"GRPC\""
        );
        assert_eq!(
            serde_json::to_string(&TransportProtocol::Rest).expect("ser"),
            "\"REST\""
        );
    }

    #[test]
    fn transport_protocol_deserialization() {
        let p: TransportProtocol = serde_json::from_str("\"JSONRPC\"").expect("deser");
        assert_eq!(p, TransportProtocol::JsonRpc);
    }

    #[test]
    fn agent_card_roundtrip() {
        let card = minimal_card();
        let json = serde_json::to_string(&card).expect("serialize");
        assert!(json.contains("\"protocolVersion\":\"0.3.0\""));
        assert!(json.contains("\"preferredTransport\":\"JSONRPC\""));

        let back: AgentCard = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name, "Test Agent");
        assert_eq!(back.preferred_transport, TransportProtocol::JsonRpc);
    }

    #[test]
    fn optional_fields_omitted() {
        let card = minimal_card();
        let json = serde_json::to_string(&card).expect("serialize");
        assert!(!json.contains("\"provider\""), "provider should be absent");
        assert!(!json.contains("\"iconUrl\""), "iconUrl should be absent");
        assert!(
            !json.contains("\"securitySchemes\""),
            "securitySchemes should be absent"
        );
    }
}
