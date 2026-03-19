// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Agent card and capability discovery types.
//!
//! The [`AgentCard`] is the root discovery document served by an A2A agent at
//! `/.well-known/agent.json`. It describes the agent's identity,
//! capabilities, skills, security requirements, and supported interfaces.
//!
//! # v1.0 changes
//!
//! - `url` and `preferred_transport` replaced by `supported_interfaces`
//! - `protocol_version` moved from `AgentCard` to `AgentInterface`
//! - `AgentInterface.transport` renamed to `protocol_binding`
//! - `supports_authenticated_extended_card` moved to `AgentCapabilities.extended_agent_card`
//! - Security fields renamed to `security_requirements`

use serde::{Deserialize, Serialize};

use crate::extensions::{AgentCardSignature, AgentExtension};
use crate::security::{NamedSecuritySchemes, SecurityRequirement};

// ── AgentInterface ────────────────────────────────────────────────────────────

/// A transport interface offered by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInterface {
    /// Base URL of this interface endpoint.
    pub url: String,

    /// Protocol binding identifier (e.g. `"JSONRPC"`, `"REST"`, `"GRPC"`).
    pub protocol_binding: String,

    /// A2A protocol version string (e.g. `"1.0.0"`).
    pub protocol_version: String,

    /// Optional tenant identifier for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}

// ── AgentCapabilities ─────────────────────────────────────────────────────────

/// Optional capability flags advertised by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AgentCapabilities {
    /// Whether the agent supports streaming via `SendStreamingMessage`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,

    /// Whether the agent supports push notification delivery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_notifications: Option<bool>,

    /// Whether this agent serves an authenticated extended card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended_agent_card: Option<bool>,

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
            extended_agent_card: None,
            extensions: None,
        }
    }

    /// Sets the streaming capability flag.
    #[must_use]
    pub const fn with_streaming(mut self, streaming: bool) -> Self {
        self.streaming = Some(streaming);
        self
    }

    /// Sets the push notifications capability flag.
    #[must_use]
    pub const fn with_push_notifications(mut self, push: bool) -> Self {
        self.push_notifications = Some(push);
        self
    }

    /// Sets the extended agent card capability flag.
    #[must_use]
    pub const fn with_extended_agent_card(mut self, extended: bool) -> Self {
        self.extended_agent_card = Some(extended);
        self
    }
}

impl Default for AgentCapabilities {
    fn default() -> Self {
        Self::none()
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
    pub security_requirements: Option<Vec<SecurityRequirement>>,
}

// ── AgentCard ─────────────────────────────────────────────────────────────────

/// The root discovery document for an A2A agent.
///
/// Served at `/.well-known/agent.json`. Clients fetch this document to
/// discover the agent's interfaces, capabilities, skills, and security
/// requirements before establishing a session.
///
/// In v1.0, `protocol_version` and `url` moved to [`AgentInterface`], and
/// `supported_interfaces` replaces the old `url`/`preferred_transport`/
/// `additional_interfaces` fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    /// Display name of the agent.
    pub name: String,

    /// Primary URL of the agent.
    ///
    /// Convenience field that typically matches the URL of the first
    /// entry in `supported_interfaces`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Human-readable description of the agent's purpose.
    pub description: String,

    /// Semantic version of this agent implementation.
    pub version: String,

    /// Transport interfaces offered by this agent.
    ///
    /// **Spec requirement:** Must contain at least one element.
    pub supported_interfaces: Vec<AgentInterface>,

    /// Default MIME types accepted as input.
    pub default_input_modes: Vec<String>,

    /// Default MIME types produced as output.
    pub default_output_modes: Vec<String>,

    /// Skills offered by this agent.
    ///
    /// **Spec requirement:** Must contain at least one element.
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
    pub security_requirements: Option<Vec<SecurityRequirement>>,

    /// Cryptographic signatures over this card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signatures: Option<Vec<AgentCardSignature>>,
}

impl AgentCard {
    /// Validates the agent card for completeness.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `name` is empty
    /// - `supported_interfaces` is empty (spec requires at least one interface)
    pub const fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            return Err("agent card name must not be empty");
        }
        if self.supported_interfaces.is_empty() {
            return Err("agent card must have at least one supported interface");
        }
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_card() -> AgentCard {
        AgentCard {
            url: None,
            name: "Test Agent".into(),
            description: "A test agent".into(),
            version: "1.0.0".into(),
            supported_interfaces: vec![AgentInterface {
                url: "https://agent.example.com/rpc".into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            }],
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
                security_requirements: None,
            }],
            capabilities: AgentCapabilities::none(),
            provider: None,
            icon_url: None,
            documentation_url: None,
            security_schemes: None,
            security_requirements: None,
            signatures: None,
        }
    }

    #[test]
    fn agent_card_roundtrip() {
        let card = minimal_card();
        let json = serde_json::to_string(&card).expect("serialize");
        assert!(json.contains("\"supportedInterfaces\""));
        assert!(json.contains("\"protocolBinding\":\"JSONRPC\""));
        assert!(json.contains("\"protocolVersion\":\"1.0.0\""));
        assert!(
            !json.contains("\"preferredTransport\""),
            "v1.0 removed this field"
        );

        let back: AgentCard = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name, "Test Agent");
        assert_eq!(back.supported_interfaces[0].protocol_binding, "JSONRPC");
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

    #[test]
    fn extended_agent_card_in_capabilities() {
        let mut card = minimal_card();
        card.capabilities.extended_agent_card = Some(true);
        let json = serde_json::to_string(&card).expect("serialize");
        assert!(json.contains("\"extendedAgentCard\":true"));
    }

    #[test]
    fn wire_format_security_requirements_field_name() {
        use crate::security::{SecurityRequirement, StringList};
        use std::collections::HashMap;

        let mut card = minimal_card();
        card.security_requirements = Some(vec![SecurityRequirement {
            schemes: HashMap::from([("bearer".into(), StringList { list: vec![] })]),
        }]);
        let json = serde_json::to_string(&card).unwrap();
        // Must use "securityRequirements" (not "security")
        assert!(
            json.contains("\"securityRequirements\""),
            "field must be securityRequirements: {json}"
        );
        assert!(
            !json.contains("\"security\":"),
            "must not have bare 'security' field: {json}"
        );
    }

    #[test]
    fn wire_format_skill_security_requirements() {
        use crate::security::{SecurityRequirement, StringList};
        use std::collections::HashMap;

        let skill = AgentSkill {
            id: "s1".into(),
            name: "Skill".into(),
            description: "A skill".into(),
            tags: vec![],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: Some(vec![SecurityRequirement {
                schemes: HashMap::from([(
                    "oauth2".into(),
                    StringList {
                        list: vec!["read".into()],
                    },
                )]),
            }]),
        };
        let json = serde_json::to_string(&skill).unwrap();
        assert!(
            json.contains("\"securityRequirements\""),
            "skill must use securityRequirements: {json}"
        );
    }

    #[test]
    fn wire_format_capabilities_no_state_transition_history() {
        let card = minimal_card();
        let json = serde_json::to_string(&card).unwrap();
        assert!(
            !json.contains("stateTransitionHistory"),
            "stateTransitionHistory must not appear: {json}"
        );
    }

    // ── AgentCapabilities builder tests ───────────────────────────────────

    #[test]
    fn capabilities_none_all_fields_unset() {
        let caps = AgentCapabilities::none();
        assert!(caps.streaming.is_none());
        assert!(caps.push_notifications.is_none());
        assert!(caps.extended_agent_card.is_none());
        assert!(caps.extensions.is_none());
    }

    #[test]
    fn capabilities_default_equals_none() {
        let def = AgentCapabilities::default();
        let none = AgentCapabilities::none();
        assert_eq!(def.streaming, none.streaming);
        assert_eq!(def.push_notifications, none.push_notifications);
        assert_eq!(def.extended_agent_card, none.extended_agent_card);
    }

    #[test]
    fn capabilities_with_streaming_sets_field() {
        let caps = AgentCapabilities::none().with_streaming(true);
        assert_eq!(caps.streaming, Some(true));
        assert!(caps.push_notifications.is_none());
        assert!(caps.extended_agent_card.is_none());

        let caps = AgentCapabilities::none().with_streaming(false);
        assert_eq!(caps.streaming, Some(false));
    }

    #[test]
    fn capabilities_with_push_notifications_sets_field() {
        let caps = AgentCapabilities::none().with_push_notifications(true);
        assert_eq!(caps.push_notifications, Some(true));
        assert!(caps.streaming.is_none());
        assert!(caps.extended_agent_card.is_none());

        let caps = AgentCapabilities::none().with_push_notifications(false);
        assert_eq!(caps.push_notifications, Some(false));
    }

    #[test]
    fn capabilities_with_extended_agent_card_sets_field() {
        let caps = AgentCapabilities::none().with_extended_agent_card(true);
        assert_eq!(caps.extended_agent_card, Some(true));
        assert!(caps.streaming.is_none());
        assert!(caps.push_notifications.is_none());

        let caps = AgentCapabilities::none().with_extended_agent_card(false);
        assert_eq!(caps.extended_agent_card, Some(false));
    }

    #[test]
    fn capabilities_builder_chaining() {
        let caps = AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false)
            .with_extended_agent_card(true);
        assert_eq!(caps.streaming, Some(true));
        assert_eq!(caps.push_notifications, Some(false));
        assert_eq!(caps.extended_agent_card, Some(true));
    }

    // ── AgentCard::validate tests ─────────────────────────────────────────

    #[test]
    fn validate_minimal_card_ok() {
        let card = minimal_card();
        assert!(card.validate().is_ok());
    }

    #[test]
    fn validate_empty_name_returns_error() {
        let mut card = minimal_card();
        card.name = String::new();
        let err = card.validate().unwrap_err();
        assert!(err.contains("name"), "error should mention name: {err}");
    }

    #[test]
    fn validate_empty_supported_interfaces_returns_error() {
        let mut card = minimal_card();
        card.supported_interfaces = vec![];
        let err = card.validate().unwrap_err();
        assert!(
            err.contains("supported interface"),
            "error should mention supported interface: {err}"
        );
    }
}
