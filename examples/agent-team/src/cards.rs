// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Agent card builders for each agent in the team.

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};

pub fn code_analyzer_card(url: &str) -> AgentCard {
    AgentCard {
        name: "Code Analyzer".into(),
        description: "Analyzes code: LOC, complexity, metrics".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into(), "application/json".into()],
        skills: vec![AgentSkill {
            id: "analyze".into(),
            name: "Code Analysis".into(),
            description: "Counts lines, words, chars and assesses complexity".into(),
            tags: vec!["code".into(), "analysis".into(), "metrics".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

pub fn build_monitor_card(url: &str) -> AgentCard {
    AgentCard {
        name: "Build Monitor".into(),
        description: "Runs cargo builds and streams output".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "REST".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "build".into(),
            name: "Build Runner".into(),
            description: "Runs cargo check/build/test".into(),
            tags: vec!["build".into(), "cargo".into(), "ci".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

pub fn health_monitor_card(url: &str) -> AgentCard {
    AgentCard {
        name: "Health Monitor".into(),
        description: "Monitors agent team health and connectivity".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into(), "application/json".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "health-check".into(),
            name: "Health Check".into(),
            description: "Pings agents and reports their status".into(),
            tags: vec!["health".into(), "monitoring".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

pub fn coordinator_card(url: &str) -> AgentCard {
    AgentCard {
        name: "Coordinator".into(),
        description: "Orchestrates the agent team via A2A calls".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "REST".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "orchestrate".into(),
            name: "Team Orchestration".into(),
            description: "Delegates tasks to specialized agents and aggregates results".into(),
            tags: vec!["orchestration".into(), "delegation".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}
