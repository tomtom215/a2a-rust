// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Reusable test fixtures for benchmarks.
//!
//! Every helper returns a deterministic value so that criterion's statistical
//! analysis is not polluted by randomized IDs across iterations.

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

// ── Messages ────────────────────────────────────────────────────────────────

/// Creates a minimal user message with deterministic IDs.
pub fn user_message(text: &str) -> Message {
    Message {
        id: MessageId::new("msg-bench-001"),
        role: MessageRole::User,
        parts: vec![Part::text(text)],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

/// Creates a multi-part user message with text and a file URL.
pub fn multi_part_message() -> Message {
    Message {
        id: MessageId::new("msg-bench-multi"),
        role: MessageRole::User,
        parts: vec![
            Part::text("Benchmark message with multiple parts for overhead testing."),
            Part::url("https://example.com/doc.pdf"),
        ],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: Some(serde_json::json!({"source": "benchmark", "iteration": 0})),
    }
}

// ── Tasks ───────────────────────────────────────────────────────────────────

/// Creates a completed task with a single-message history.
pub fn completed_task(id: usize) -> Task {
    Task {
        id: TaskId::new(format!("task-bench-{id:06}")),
        context_id: ContextId::new("ctx-bench-001"),
        status: TaskStatus::new(TaskState::Completed),
        history: Some(vec![user_message("Hello, agent!")]),
        artifacts: Some(vec![Artifact::new(
            "echo-artifact",
            vec![Part::text("Echo: Hello, agent!")],
        )]),
        metadata: None,
    }
}

/// Creates a minimal working-state task (no history/artifacts).
pub fn minimal_task(id: usize) -> Task {
    Task {
        id: TaskId::new(format!("task-min-{id:06}")),
        context_id: ContextId::new("ctx-bench-001"),
        status: TaskStatus::new(TaskState::Working),
        history: None,
        artifacts: None,
        metadata: None,
    }
}

// ── Stream events ───────────────────────────────────────────────────────────

/// Creates a status-update stream event.
pub fn status_event(task_id: &str, state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new(task_id),
        context_id: ContextId::new("ctx-bench-001"),
        status: TaskStatus::new(state),
        metadata: None,
    })
}

/// Creates an artifact-update stream event.
pub fn artifact_event(task_id: &str) -> StreamResponse {
    StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
        task_id: TaskId::new(task_id),
        context_id: ContextId::new("ctx-bench-001"),
        artifact: Artifact::new("bench-artifact", vec![Part::text("Echo: benchmark")]),
        append: None,
        last_chunk: Some(true),
        metadata: None,
    })
}

// ── Agent card ──────────────────────────────────────────────────────────────

/// Creates an agent card with the given interface URL.
pub fn agent_card(url: &str) -> AgentCard {
    AgentCard {
        url: Some(url.to_owned()),
        name: "Bench Agent".into(),
        description: "Benchmark agent — trivial echo for SDK perf tests".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.to_owned(),
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
            tags: vec!["echo".into(), "benchmark".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none().with_streaming(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

// ── Send params ─────────────────────────────────────────────────────────────

/// Creates `MessageSendParams` for a synchronous send.
pub fn send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        context_id: None,
        message: user_message(text),
        configuration: None,
        metadata: None,
    }
}

/// Creates `MessageSendParams` targeting an existing context (for multi-turn).
pub fn send_params_with_context(text: &str, context_id: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        context_id: Some(context_id.into()),
        message: user_message(text),
        configuration: None,
        metadata: None,
    }
}

// ── Realistic payloads ──────────────────────────────────────────────────────

/// Creates a deeply nested JSON metadata object (depth levels of nesting).
pub fn nested_metadata(depth: usize) -> serde_json::Value {
    let mut val = serde_json::json!({"leaf": "value", "count": 42});
    for i in (0..depth).rev() {
        val = serde_json::json!({
            format!("level_{i}"): val,
            "sibling": format!("data-at-level-{i}"),
        });
    }
    val
}

/// Creates a message with large metadata (approximately `kb` kilobytes).
pub fn large_metadata_message(kb: usize) -> Message {
    // Build metadata with repeated keys to reach target size.
    let mut map = serde_json::Map::new();
    let chunk = "x".repeat(100); // ~100 bytes per entry
    let entries = (kb * 1024) / 120; // ~120 bytes per JSON entry with key
    for i in 0..entries {
        map.insert(
            format!("field_{i:06}"),
            serde_json::Value::String(chunk.clone()),
        );
    }
    Message {
        id: MessageId::new("msg-bench-large-meta"),
        role: MessageRole::User,
        parts: vec![Part::text("Message with large metadata payload")],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: Some(serde_json::Value::Object(map)),
    }
}

/// Creates a message with mixed parts: text, file URL, and inline data.
pub fn mixed_parts_message() -> Message {
    Message {
        id: MessageId::new("msg-bench-mixed"),
        role: MessageRole::User,
        parts: vec![
            Part::text("Analyze this document and the attached data."),
            Part::url("https://example.com/reports/q4-2025.pdf"),
            Part::text("Additional context: the report covers revenue projections."),
        ],
        task_id: None,
        context_id: None,
        reference_task_ids: Some(vec!["task-prev-001".into(), "task-prev-002".into()]),
        extensions: None,
        metadata: Some(serde_json::json!({
            "source": "benchmark",
            "priority": "high",
            "tags": ["finance", "q4", "projections"],
            "requester": {
                "name": "Bench User",
                "department": "Engineering",
                "access_level": 3,
            },
        })),
    }
}

/// Creates a complex agent card with many skills (simulates production agents).
pub fn complex_agent_card(url: &str, skill_count: usize) -> AgentCard {
    let skills: Vec<AgentSkill> = (0..skill_count)
        .map(|i| AgentSkill {
            id: format!("skill-{i:04}"),
            name: format!("Skill {i}"),
            description: format!("Performs operation #{i} with configurable parameters"),
            tags: vec![
                format!("category-{}", i % 10),
                format!("tier-{}", i % 3),
                "production".into(),
            ],
            examples: Some(vec![
                format!("Run skill {i} on input data"),
                format!("Execute operation {i}"),
            ]),
            input_modes: Some(vec!["text/plain".into(), "application/json".into()]),
            output_modes: Some(vec!["text/plain".into()]),
            security_requirements: None,
        })
        .collect();

    AgentCard {
        url: Some(url.to_owned()),
        name: "Production Agent".into(),
        description: "A production agent with many skills for benchmark testing".into(),
        version: "2.1.0".into(),
        supported_interfaces: vec![
            AgentInterface {
                url: url.to_owned(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            },
            AgentInterface {
                url: format!("{url}/rest"),
                protocol_binding: "REST".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            },
        ],
        default_input_modes: vec!["text/plain".into(), "application/json".into()],
        default_output_modes: vec!["text/plain".into(), "application/json".into()],
        skills,
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
        provider: None,
        icon_url: Some("https://example.com/agent-icon.png".into()),
        documentation_url: Some("https://docs.example.com/agent".into()),
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// Creates a completed task with N messages in its history (simulates
/// a multi-turn conversation).
pub fn task_with_history(id: usize, turns: usize) -> Task {
    let history: Vec<Message> = (0..turns)
        .map(|t| {
            let role = if t % 2 == 0 {
                MessageRole::User
            } else {
                MessageRole::Agent
            };
            Message {
                id: MessageId::new(format!("msg-{id}-turn-{t}")),
                role,
                parts: vec![Part::text(format!(
                    "Turn {t}: This is a realistic conversation message with enough text \
                     to simulate real-world message sizes in agent interactions."
                ))],
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            }
        })
        .collect();

    Task {
        id: TaskId::new(format!("task-conv-{id:06}")),
        context_id: ContextId::new(format!("ctx-conv-{id:06}")),
        status: TaskStatus::new(TaskState::Completed),
        history: Some(history),
        artifacts: Some(vec![Artifact::new(
            "result",
            vec![Part::text("Final agent response for this conversation.")],
        )]),
        metadata: Some(serde_json::json!({"turns": turns, "model": "benchmark"})),
    }
}
