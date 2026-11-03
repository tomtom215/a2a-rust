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
    Message::new(
        MessageId::new("msg-bench-001"),
        MessageRole::User,
        vec![Part::text(text)],
    )
}

/// Creates a multi-part user message with text and a file URL.
pub fn multi_part_message() -> Message {
    Message::new(
        MessageId::new("msg-bench-multi"),
        MessageRole::User,
        vec![
            Part::text("Benchmark message with multiple parts for overhead testing."),
            Part::url("https://example.com/doc.pdf"),
        ],
    )
    .with_metadata(serde_json::json!({"source": "benchmark", "iteration": 0}))
}

// ── Tasks ───────────────────────────────────────────────────────────────────

/// Creates a completed task with a single-message history.
pub fn completed_task(id: usize) -> Task {
    Task::new(
        TaskId::new(format!("task-bench-{id:06}")),
        ContextId::new("ctx-bench-001"),
        TaskStatus::new(TaskState::Completed),
    )
    .with_history(vec![user_message("Hello, agent!")])
    .with_artifacts(vec![Artifact::new(
        "echo-artifact",
        vec![Part::text("Echo: Hello, agent!")],
    )])
}

/// Creates a minimal working-state task (no history/artifacts).
pub fn minimal_task(id: usize) -> Task {
    Task::new(
        TaskId::new(format!("task-min-{id:06}")),
        ContextId::new("ctx-bench-001"),
        TaskStatus::new(TaskState::Working),
    )
}

// ── Stream events ───────────────────────────────────────────────────────────

/// Creates a status-update stream event.
pub fn status_event(task_id: &str, state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new(task_id),
        ContextId::new("ctx-bench-001"),
        TaskStatus::new(state),
    ))
}

/// Creates an artifact-update stream event.
pub fn artifact_event(task_id: &str) -> StreamResponse {
    StreamResponse::ArtifactUpdate(
        TaskArtifactUpdateEvent::new(
            TaskId::new(task_id),
            ContextId::new("ctx-bench-001"),
            Artifact::new("bench-artifact", vec![Part::text("Echo: benchmark")]),
        )
        .with_last_chunk(true),
    )
}

// ── Agent card ──────────────────────────────────────────────────────────────

/// Creates an agent card with the given interface URL.
pub fn agent_card(url: &str) -> AgentCard {
    {
        let mut built = AgentCard::new(
            "Bench Agent",
            "1.0.0",
            AgentInterface::new(url.to_owned(), "JSONRPC").with_protocol_version("1.0.0"),
        )
        .with_description("Benchmark agent — trivial echo for SDK perf tests")
        .with_input_modes(vec!["text/plain"])
        .with_output_modes(vec!["text/plain"])
        .with_skill(
            AgentSkill::new("echo", "Echo", "Echoes input").with_tags(vec!["echo", "benchmark"]),
        )
        .with_capabilities(AgentCapabilities::none().with_streaming(true));
        built.url = Some(url.to_owned());
        built
    }
}

// ── Send params ─────────────────────────────────────────────────────────────

/// Creates `MessageSendParams` for a synchronous send.
pub fn send_params(text: &str) -> MessageSendParams {
    MessageSendParams::new(user_message(text))
}

/// Creates `MessageSendParams` targeting an existing context (for multi-turn).
pub fn send_params_with_context(text: &str, context_id: &str) -> MessageSendParams {
    let mut msg = user_message(text);
    msg.context_id = Some(ContextId::new(context_id));
    MessageSendParams::new(msg)
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
    let chunk: serde_json::Value = serde_json::Value::String("x".repeat(100)); // ~100 bytes per entry
    let entries = (kb * 1024) / 120; // ~120 bytes per JSON entry with key
    for i in 0..entries {
        map.insert(format!("field_{i:06}"), chunk.clone());
    }
    Message::new(
        MessageId::new("msg-bench-large-meta"),
        MessageRole::User,
        vec![Part::text("Message with large metadata payload")],
    )
    .with_metadata(serde_json::Value::Object(map))
}

/// Creates a message with mixed parts: text, file URL, and inline data.
pub fn mixed_parts_message() -> Message {
    Message::new(
        MessageId::new("msg-bench-mixed"),
        MessageRole::User,
        vec![
            Part::text("Analyze this document and the attached data."),
            Part::url("https://example.com/reports/q4-2025.pdf"),
            Part::text("Additional context: the report covers revenue projections."),
        ],
    )
    .with_reference_task_ids(vec!["task-prev-001", "task-prev-002"])
    .with_metadata(serde_json::json!({
        "source": "benchmark",
        "priority": "high",
        "tags": ["finance", "q4", "projections"],
        "requester": {
            "name": "Bench User",
            "department": "Engineering",
            "access_level": 3,
        },
    }))
}

/// Creates a complex agent card with many skills (simulates production agents).
pub fn complex_agent_card(url: &str, skill_count: usize) -> AgentCard {
    let skills: Vec<AgentSkill> = (0..skill_count)
        .map(|i| {
            AgentSkill::new(
                format!("skill-{i:04}"),
                format!("Skill {i}"),
                format!("Performs operation #{i} with configurable parameters"),
            )
            .with_tags(vec![
                format!("category-{}", i % 10),
                format!("tier-{}", i % 3),
                "production".into(),
            ])
            .with_examples(vec![
                format!("Run skill {i} on input data"),
                format!("Execute operation {i}"),
            ])
            .with_input_modes(vec!["text/plain", "application/json"])
            .with_output_modes(vec!["text/plain"])
        })
        .collect();

    {
        let mut built = AgentCard::new(
            "Production Agent",
            "2.1.0",
            AgentInterface::jsonrpc(String::new()),
        );
        built.url = Some(url.to_owned());
        built.description = "A production agent with many skills for benchmark testing".into();
        built.supported_interfaces = vec![
            AgentInterface::new(url.to_owned(), "JSONRPC").with_protocol_version("1.0.0"),
            AgentInterface::new(format!("{url}/rest"), "REST").with_protocol_version("1.0.0"),
        ];
        built.default_input_modes = vec!["text/plain".into(), "application/json".into()];
        built.default_output_modes = vec!["text/plain".into(), "application/json".into()];
        built.skills = skills;
        built.capabilities = AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true);
        built.provider = None;
        built.icon_url = Some("https://example.com/agent-icon.png".into());
        built.documentation_url = Some("https://docs.example.com/agent".into());
        built.security_schemes = None;
        built.security_requirements = None;
        built.signatures = None;
        built
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
            Message::new(
                MessageId::new(format!("msg-{id}-turn-{t}")),
                role,
                vec![Part::text(format!(
                    "Turn {t}: This is a realistic conversation message with enough text \
                     to simulate real-world message sizes in agent interactions."
                ))],
            )
        })
        .collect();

    Task::new(
        TaskId::new(format!("task-conv-{id:06}")),
        ContextId::new(format!("ctx-conv-{id:06}")),
        TaskStatus::new(TaskState::Completed),
    )
    .with_history(history)
    .with_artifacts(vec![Artifact::new(
        "result",
        vec![Part::text("Final agent response for this conversation.")],
    )])
    .with_metadata(serde_json::json!({"turns": turns, "model": "benchmark"}))
}
