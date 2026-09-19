// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The two tools this example's agent can call, and the dispatch that runs
//! them.
//!
//! **A2A has no tool concept.** The protocol carries messages, tasks and
//! artifacts *between* agents and says nothing about what an agent does
//! inside one. Tool calling happens a layer down, between an executor and its
//! model, and the model provider defines the shape — here `rig-core`'s
//! [`ToolDefinition`]. Nothing in this module touches `a2a-protocol-*`, and
//! that separation is the thing worth copying: an A2A server does not become
//! a tool runtime, it hosts one.
//!
//! Both tools read one in-memory table and reach no network, so
//! [`crate::agent`]'s loop is exercisable with no provider, no fixtures and
//! no key. The service names match `examples/incident-response`'s runbooks so
//! the two examples describe the same imaginary system.
//!
//! # The catalogue is two tools on purpose
//!
//! [`LIST_SERVICES`] takes no arguments and [`SERVICE_STATUS`] takes one, so
//! between them they cover both JSON Schema shapes a provider has to encode.
//! They also compose: a model given a name it does not recognise has to
//! discover the inventory before it can query it, which is a *two*-round tool
//! loop. A single-tool catalogue only ever demonstrates one round, and one
//! round is the case that works even when the loop is written wrong.

use std::fmt;

use rig_core::completion::ToolDefinition;
use serde_json::{Value, json};

/// Name of the zero-argument discovery tool.
pub const LIST_SERVICES: &str = "list_services";

/// Name of the single-argument lookup tool.
pub const SERVICE_STATUS: &str = "service_status";

/// One row of the inventory the tools read.
struct Service {
    name: &'static str,
    state: &'static str,
    version: &'static str,
    uptime_s: u64,
}

/// The whole backing store. A real agent would reach a database or an API
/// here; the point of the example is the loop above it, not the data under
/// it, and a constant keeps every test deterministic.
const INVENTORY: &[Service] = &[
    Service {
        name: "payments-api",
        state: "degraded",
        version: "4.2.1",
        uptime_s: 1_814_400,
    },
    Service {
        name: "checkout",
        state: "healthy",
        version: "2.8.0",
        uptime_s: 259_200,
    },
    Service {
        name: "search",
        state: "healthy",
        version: "1.19.3",
        uptime_s: 7_776_000,
    },
];

/// The tool catalogue, as the model is told about it.
///
/// Sent on *every* request in the loop, not just the first: a provider is
/// stateless across turns, so a request that omits the catalogue is a request
/// in which the model cannot call anything.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: LIST_SERVICES.to_owned(),
            description: "List the name of every service in the inventory. \
                 Call this first when the caller names a service you do not recognise."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: SERVICE_STATUS.to_owned(),
            description: "Look up one service's current state, deployed version, \
                 and uptime in seconds."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "service": {
                        "type": "string",
                        "description": "Exact service name, as returned by list_services."
                    }
                },
                "required": ["service"],
                "additionalProperties": false
            }),
        },
    ]
}

/// Why a tool call produced no result.
///
/// Every variant travels back to the model as that call's result rather than
/// failing the A2A task — see [`crate::agent::RigAgent::prompt`]. A model that
/// asked for a service which does not exist can recover by calling
/// [`LIST_SERVICES`]; a failed task cannot.
#[derive(Debug, PartialEq, Eq)]
pub enum ToolError {
    /// The model named a tool that is not in [`definitions`].
    UnknownTool(String),
    /// A required argument was absent, or was not the declared type.
    MissingArgument {
        /// The tool that was called.
        tool: &'static str,
        /// The argument it needed.
        argument: &'static str,
    },
    /// The named service is not in the inventory.
    UnknownService(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTool(name) => {
                write!(f, "no tool named '{name}'")
            }
            Self::MissingArgument { tool, argument } => {
                write!(f, "'{tool}' needs a string argument '{argument}'")
            }
            Self::UnknownService(name) => write!(
                f,
                "no service named '{name}'; call {LIST_SERVICES} for the inventory"
            ),
        }
    }
}

impl std::error::Error for ToolError {}

/// Runs one tool call and returns its output as the text the model will read.
///
/// `arguments` is what the model sent, already normalised. A provider may put
/// tool arguments on the wire either as a JSON object or as a *string* holding
/// JSON, and rig's OpenAI decoder collapses both into one `Value` before this
/// is reached — verified against rig-core 0.42.0's own
/// `deserialize_llama_cpp_tool_call` test and its string-form sibling
/// (`src/providers/openai/completion/mod.rs`), which assert the identical
/// `json!({"city": "Paris"})` from both wires. So this function sees an object
/// either way and re-parses nothing.
pub fn invoke(name: &str, arguments: &Value) -> Result<String, ToolError> {
    match name {
        LIST_SERVICES => {
            let names: Vec<&str> = INVENTORY.iter().map(|service| service.name).collect();
            Ok(Value::from(names).to_string())
        }
        SERVICE_STATUS => {
            let wanted = arguments.get("service").and_then(Value::as_str).ok_or(
                ToolError::MissingArgument {
                    tool: SERVICE_STATUS,
                    argument: "service",
                },
            )?;
            let found = INVENTORY
                .iter()
                .find(|service| service.name == wanted)
                .ok_or_else(|| ToolError::UnknownService(wanted.to_owned()))?;
            Ok(json!({
                "service": found.name,
                "state": found.state,
                "version": found.version,
                "uptimeSeconds": found.uptime_s,
            })
            .to_string())
        }
        other => Err(ToolError::UnknownTool(other.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::{LIST_SERVICES, SERVICE_STATUS, ToolError, definitions, invoke};
    use serde_json::{Value, json};

    #[test]
    fn every_declared_tool_is_dispatchable() {
        // A catalogue entry the dispatch does not know is a tool the model
        // will call and never get an answer from, which no test of either
        // half alone would catch.
        for tool in definitions() {
            let outcome = invoke(&tool.name, &json!({ "service": "checkout" }));
            assert!(
                !matches!(outcome, Err(ToolError::UnknownTool(_))),
                "declared tool '{}' is not dispatchable",
                tool.name
            );
        }
    }

    #[test]
    fn every_declared_schema_is_an_object_schema() {
        for tool in definitions() {
            assert_eq!(
                tool.parameters.get("type").and_then(Value::as_str),
                Some("object"),
                "'{}' must declare an object schema — providers reject any other \
                 top-level type for function parameters",
                tool.name
            );
            assert!(
                !tool.description.is_empty(),
                "'{}' has no description, which is the only thing telling the \
                 model when to call it",
                tool.name
            );
        }
    }

    #[test]
    fn listing_returns_every_service_in_the_inventory() {
        let listed = invoke(LIST_SERVICES, &json!({})).expect("list_services takes no arguments");
        let names: Vec<String> = serde_json::from_str(&listed).expect("the tool returns JSON");
        assert_eq!(names, ["payments-api", "checkout", "search"]);
    }

    #[test]
    fn a_status_lookup_returns_the_row() {
        let found = invoke(SERVICE_STATUS, &json!({ "service": "payments-api" }))
            .expect("payments-api is in the inventory");
        let row: Value = serde_json::from_str(&found).expect("the tool returns JSON");
        assert_eq!(row["service"], "payments-api");
        assert_eq!(row["state"], "degraded");
        assert_eq!(row["version"], "4.2.1");
        assert_eq!(row["uptimeSeconds"], 1_814_400);
    }

    #[test]
    fn an_unknown_service_names_the_discovery_tool() {
        let refused = invoke(SERVICE_STATUS, &json!({ "service": "billing" }))
            .expect_err("billing is not in the inventory");
        assert_eq!(refused, ToolError::UnknownService("billing".to_owned()));
        // The message is what the model reads, so it has to say what to do
        // next rather than only what went wrong.
        assert!(refused.to_string().contains(LIST_SERVICES));
    }

    #[test]
    fn a_missing_argument_is_refused_rather_than_defaulted() {
        assert_eq!(
            invoke(SERVICE_STATUS, &json!({})).expect_err("no service argument"),
            ToolError::MissingArgument {
                tool: SERVICE_STATUS,
                argument: "service",
            }
        );
        // A non-string is the same failure: silently coercing 7 to "7" would
        // turn a model's type error into a confusing UnknownService.
        assert_eq!(
            invoke(SERVICE_STATUS, &json!({ "service": 7 })).expect_err("wrong type"),
            ToolError::MissingArgument {
                tool: SERVICE_STATUS,
                argument: "service",
            }
        );
    }

    #[test]
    fn an_unknown_tool_is_refused() {
        assert_eq!(
            invoke("restart_everything", &json!({})).expect_err("not a declared tool"),
            ToolError::UnknownTool("restart_everything".to_owned())
        );
    }
}
