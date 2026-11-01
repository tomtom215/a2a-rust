// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Agent card builders for each agent in the team.

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};

pub fn code_analyzer_card(url: &str) -> AgentCard {
    AgentCard::new(
        "Code Analyzer",
        "1.0.0",
        AgentInterface::new(url, "JSONRPC"),
    )
    .with_description("Analyzes code: LOC, complexity, metrics")
    .with_input_modes(vec!["text/plain"])
    .with_output_modes(vec!["text/plain", "application/json"])
    .with_skill(
        AgentSkill::new(
            "analyze",
            "Code Analysis",
            "Counts lines, words, chars and assesses complexity",
        )
        .with_tags(vec!["code", "analysis", "metrics"]),
    )
    // `extended_agent_card` is on because Test 68 drives
    // `GetExtendedAgentCard` against this agent. Until 2026-08-11 the
    // capability was absent while the test existed, so the handler
    // correctly answered `UnsupportedOperation` (spec §3.1.11) and the
    // test could never have passed — the suite carried a permanently red
    // check for a feature it had never configured.
    .with_capabilities(
        AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false)
            .with_extended_agent_card(true),
    )
}

pub fn build_monitor_card(url: &str) -> AgentCard {
    AgentCard::new(
        "Build Monitor",
        "1.0.0",
        AgentInterface::new(url, "HTTP+JSON"),
    )
    .with_description("Runs cargo builds and streams output")
    .with_input_modes(vec!["text/plain"])
    .with_output_modes(vec!["text/plain"])
    .with_skill(
        AgentSkill::new("build", "Build Runner", "Runs cargo check/build/test")
            .with_tags(vec!["build", "cargo", "ci"]),
    )
    .with_capabilities(
        AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
    )
}

pub fn health_monitor_card(url: &str) -> AgentCard {
    AgentCard::new(
        "Health Monitor",
        "1.0.0",
        AgentInterface::new(url, "JSONRPC"),
    )
    .with_description("Monitors agent team health and connectivity")
    .with_input_modes(vec!["text/plain", "application/json"])
    .with_output_modes(vec!["text/plain"])
    .with_skill(
        AgentSkill::new(
            "health-check",
            "Health Check",
            "Pings agents and reports their status",
        )
        .with_tags(vec!["health", "monitoring"]),
    )
    .with_capabilities(
        AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
    )
}

/// Agent card for the gRPC-served CodeAnalyzer agent.
#[cfg(feature = "grpc")]
pub fn grpc_analyzer_card(url: &str) -> AgentCard {
    AgentCard::new(
        "Code Analyzer (gRPC)",
        "1.0.0",
        AgentInterface::new(url, "GRPC"),
    )
    .with_description("Analyzes code via gRPC transport")
    .with_input_modes(vec!["text/plain"])
    .with_output_modes(vec!["text/plain", "application/json"])
    .with_skill(
        AgentSkill::new(
            "analyze-grpc",
            "Code Analysis (gRPC)",
            "Same as Code Analyzer but over gRPC",
        )
        .with_tags(vec!["code", "analysis", "grpc"]),
    )
    .with_capabilities(
        AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false),
    )
}

pub fn coordinator_card(url: &str) -> AgentCard {
    AgentCard::new(
        "Coordinator",
        "1.0.0",
        AgentInterface::new(url, "HTTP+JSON"),
    )
    .with_description("Orchestrates the agent team via A2A calls")
    .with_input_modes(vec!["text/plain"])
    .with_output_modes(vec!["text/plain"])
    .with_skill(
        AgentSkill::new(
            "orchestrate",
            "Team Orchestration",
            "Delegates tasks to specialized agents and aggregates results",
        )
        .with_tags(vec!["orchestration", "delegation"]),
    )
    .with_capabilities(
        AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false),
    )
}
