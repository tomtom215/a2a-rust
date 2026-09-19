// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The translation itself: an A2A agent card becomes MCP tools, and a
//! finished A2A task becomes an MCP tool result.
//!
//! Kept apart from [`crate::bridge`] because it is pure — no session, no
//! network, no clock — which is what lets the awkward parts (name collisions,
//! the states that have no clean MCP counterpart) be tested directly instead
//! of inferred from a transcript.
//!
//! # A2A skills are not MCP tools, quite
//!
//! An MCP tool has a JSON Schema for its arguments. An A2A skill has a
//! description, tags and examples, and **no argument schema at all**, because
//! A2A's calling convention is fixed: you send a `Message`. So every tool this
//! bridge publishes has the same one-string schema, and the skill's
//! description is what actually tells the caller's model when to use it.
//!
//! That is a real asymmetry, not a shortcut. Anyone bridging the other way —
//! see `examples/mcp-agent` — gets a schema per tool and can pass structured
//! arguments. This direction cannot, and pretending otherwise by inventing a
//! schema per skill would publish a contract the agent never agreed to.

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::message::PartContent;
use a2a_protocol_types::task::{Task, TaskState};
use rmcp::model::{ContentBlock, Tool};
use serde_json::{Map, Value, json};

/// The single argument every bridged tool takes.
pub const MESSAGE_ARG: &str = "message";

/// Why a card could not be turned into a tool list.
#[derive(Debug, PartialEq, Eq)]
pub enum MappingError {
    /// The card advertises no skills, so there is nothing to publish.
    NoSkills,
    /// Two skills sanitize to the same MCP tool name.
    NameCollision {
        /// The MCP tool name both skills wanted.
        tool: String,
        /// The first A2A skill id that claimed it.
        first: String,
        /// The second A2A skill id that claimed it.
        second: String,
    },
}

impl std::fmt::Display for MappingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSkills => write!(f, "the agent card advertises no skills"),
            Self::NameCollision {
                tool,
                first,
                second,
            } => write!(
                f,
                "A2A skills '{first}' and '{second}' both map to MCP tool '{tool}'"
            ),
        }
    }
}

impl std::error::Error for MappingError {}

/// Sanitizes an A2A skill id into an MCP tool name.
///
/// A2A places no character restriction on a skill id; MCP clients in practice
/// accept `[A-Za-z0-9_-]`, and a name outside that is rejected by some and
/// silently mangled by others. Anything else becomes `_`.
///
/// Collisions are therefore possible, and are refused rather than resolved:
/// a bridge that quietly renamed `a.b` and `a/b` to the same `a_b` would send
/// one skill's calls to the other, which is worse than not starting.
#[must_use]
pub fn tool_name(skill_id: &str) -> String {
    skill_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Turns an agent card into the MCP tool list this bridge publishes.
///
/// # Errors
///
/// [`MappingError::NoSkills`] if the card advertises none, and
/// [`MappingError::NameCollision`] if two skill ids sanitize alike.
pub fn tools_from_card(card: &AgentCard) -> Result<Vec<Tool>, MappingError> {
    if card.skills.is_empty() {
        return Err(MappingError::NoSkills);
    }

    let mut claimed: Vec<(String, String)> = Vec::new();
    let mut tools = Vec::with_capacity(card.skills.len());

    for skill in &card.skills {
        let name = tool_name(&skill.id);
        if let Some((_, first)) = claimed.iter().find(|(taken, _)| *taken == name) {
            return Err(MappingError::NameCollision {
                tool: name,
                first: first.clone(),
                second: skill.id.clone(),
            });
        }
        claimed.push((name.clone(), skill.id.clone()));

        // The description is the agent's own words. The bridge appends the
        // one thing a skill cannot know about itself — that it is reached
        // over the network as a remote agent, not called as a local
        // function — and rewrites nothing else, because the rest is the
        // agent's claim to make.
        let mut tool = Tool::new(
            name,
            format!(
                "{} (A2A skill '{}' on remote agent '{}'; one call is one agent task)",
                skill.description, skill.id, card.name
            ),
            std::sync::Arc::new(message_schema(skill.description.as_str())),
        );
        tool.title = Some(skill.name.clone());
        tools.push(tool);
    }

    Ok(tools)
}

/// The fixed one-argument schema every bridged tool carries.
fn message_schema(skill_description: &str) -> Map<String, Value> {
    let schema = json!({
        "type": "object",
        "properties": {
            MESSAGE_ARG: {
                "type": "string",
                "description": format!(
                    "What to ask the agent, in plain language. The skill is: {skill_description}"
                ),
            }
        },
        "required": [MESSAGE_ARG],
        "additionalProperties": false
    });
    match schema {
        Value::Object(map) => map,
        // `json!` with a literal object cannot produce anything else; the
        // match is here because `Map` is the type MCP wants and unwrapping
        // would be a panic path in a bridge that should have none.
        _ => Map::new(),
    }
}

/// Renders a settled A2A task as an MCP tool result.
///
/// Returns the content blocks and whether MCP should mark the call an error.
///
/// # The states that do not map cleanly
///
/// `Completed` is a result and `Failed`/`Rejected` are errors, which is
/// straightforward. The awkward ones are `InputRequired` and `AuthRequired`:
/// A2A pauses the task and waits for another message on the same task id,
/// and MCP *does* have a counterpart for that shape —
/// `CallToolResponse::InputRequired`, the SEP-1865 multi-round-trip path.
///
/// This bridge does **not** use it, and reports those states as errors whose
/// text says what the agent is waiting for. Wiring the two together needs the
/// bridge to hold the A2A task id across MCP calls and to translate MCP's
/// `input_responses` into an A2A continuation message — real work with a real
/// chance of being subtly wrong, and a half-built version would strand tasks
/// in `input-required` with no way to answer them. Recorded as a gap rather
/// than approximated.
#[must_use]
pub fn task_to_result(task: &Task) -> (Vec<ContentBlock>, bool) {
    let mut blocks = Vec::new();

    for artifact in task.artifacts.as_deref().unwrap_or_default() {
        for part in &artifact.parts {
            // Non-text parts are dropped rather than stringified: a caller's
            // model quoting `[object]` back at a user is worse than it not
            // seeing the part at all. `dropped_part_count` makes the omission
            // visible instead of silent.
            if let PartContent::Text(text) = &part.content {
                blocks.push(ContentBlock::text(text.clone()));
            }
        }
    }

    // The status message carries the agent's own explanation of a failure,
    // and for an interrupted task it carries the question being asked. Either
    // way it is the most useful text in the task.
    if let Some(message) = &task.status.message {
        for part in &message.parts {
            if let PartContent::Text(text) = &part.content {
                blocks.push(ContentBlock::text(text.clone()));
            }
        }
    }

    let is_error = match task.status.state {
        TaskState::Completed => false,
        TaskState::InputRequired | TaskState::AuthRequired => {
            blocks.push(ContentBlock::text(format!(
                "The agent paused this task in state {:?} and is waiting for a follow-up \
                 message on task id '{}'. This bridge cannot continue a paused task — see \
                 the mapping notes in its README.",
                task.status.state, task.id
            )));
            true
        }
        other => {
            if blocks.is_empty() {
                blocks.push(ContentBlock::text(format!(
                    "The agent ended the task in state {other:?} with no output."
                )));
            }
            true
        }
    };

    (blocks, is_error)
}

/// How many non-text parts a task carried, which [`task_to_result`] drops.
///
/// Reported by the bridge so a silent omission becomes a stated one.
#[must_use]
pub fn dropped_part_count(task: &Task) -> usize {
    task.artifacts
        .as_deref()
        .unwrap_or_default()
        .iter()
        .flat_map(|artifact| &artifact.parts)
        .filter(|part| !matches!(part.content, PartContent::Text(_)))
        .count()
}
