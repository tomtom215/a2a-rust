// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A minimal [`AgentCard`] builder for a rig-backed agent.
//!
//! The card is how an A2A client discovers what an agent can do, and getting it
//! wrong is the most common way an otherwise working agent fails interop. This
//! builder fills in the parts that follow from "a rig model answers text" and
//! leaves the rest to the caller.

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};

/// Starts a card for a rig-backed agent served at `url`.
///
/// `model` names the model only so it can appear in the description; nothing
/// reads it back. Defaults: one `chat` skill, `text/plain` in and out, and
/// streaming advertised — the SDK serves SSE for every executor, including this
/// one, so advertising it is accurate rather than aspirational.
#[must_use]
pub fn agent_card(url: &str, model: &str) -> AgentCardBuilder {
    AgentCardBuilder {
        url: url.to_owned(),
        name: "Rig Agent".to_owned(),
        description: format!("A2A agent backed by the '{model}' model via rig"),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        skills: vec![AgentSkill {
            id: "chat".to_owned(),
            name: "Chat".to_owned(),
            description: "Sends the message text to the rig model and returns the completion"
                .to_owned(),
            tags: vec!["llm".to_owned(), "rig".to_owned(), "chat".to_owned()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
    }
}

/// Builder returned by [`agent_card`].
#[derive(Debug, Clone)]
pub struct AgentCardBuilder {
    url: String,
    name: String,
    description: String,
    version: String,
    skills: Vec<AgentSkill>,
}

impl AgentCardBuilder {
    /// Sets the agent's human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the agent's description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Sets the agent's version, which otherwise reports this crate's.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Replaces the default single `chat` skill.
    ///
    /// A2A clients route on skills, so an agent that really does several things
    /// should say so here rather than leave the one generic entry.
    #[must_use]
    pub fn with_skills(mut self, skills: Vec<AgentSkill>) -> Self {
        self.skills = skills;
        self
    }

    /// Builds the card.
    ///
    /// `supported_interfaces` carries the address, which is what A2A v1.0 reads;
    /// the v0.3 top-level `url` is set for peers that still look there, and the
    /// types crate does not serialize it.
    #[must_use]
    pub fn build(self) -> AgentCard {
        AgentCard {
            url: Some(self.url.clone()),
            name: self.name,
            description: self.description,
            version: self.version,
            supported_interfaces: vec![AgentInterface {
                url: self.url,
                protocol_binding: "JSONRPC".to_owned(),
                protocol_version: a2a_protocol_types::A2A_VERSION.to_owned(),
                tenant: None,
            }],
            default_input_modes: vec!["text/plain".to_owned()],
            default_output_modes: vec!["text/plain".to_owned()],
            skills: self.skills,
            capabilities: AgentCapabilities::none().with_streaming(true),
            provider: None,
            icon_url: None,
            documentation_url: None,
            security_schemes: None,
            security_requirements: None,
            signatures: None,
        }
    }
}
