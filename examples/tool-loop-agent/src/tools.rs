// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The agent's tools: a service inventory it can list and query.
//!
//! The same imaginary system as `examples/rig-agent` (whose `tools.rs` this
//! follows), so the two examples' runs compare directly. One tool takes no
//! arguments and one takes a name, and together they make a two-round loop:
//! a model given a name the inventory does not have is told to list it, and
//! has to ask again.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_sdk::server::tool_loop::{DuplicateTool, Tool, ToolError, ToolSet};
use serde_json::{Value, json};

/// One row of the inventory.
struct Service {
    name: &'static str,
    state: &'static str,
    version: &'static str,
    uptime_s: u64,
}

/// The whole backing store; a real agent would reach an API here.
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

/// `list_services`: every name in the inventory.
pub struct ListServices;

impl Tool for ListServices {
    fn name(&self) -> &'static str {
        "list_services"
    }
    fn description(&self) -> &'static str {
        "List the name of every service in the inventory. \
         Call this first when the caller names a service you do not recognise."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }
    fn call<'a>(
        &'a self,
        _arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let names: Vec<&str> = INVENTORY.iter().map(|s| s.name).collect();
        Box::pin(async move { Ok(Value::from(names).to_string()) })
    }
}

/// `service_status`: one service's state, version and uptime.
pub struct ServiceStatus;

impl Tool for ServiceStatus {
    fn name(&self) -> &'static str {
        "service_status"
    }
    fn description(&self) -> &'static str {
        "Look up one service's current state, deployed version, and uptime in seconds."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": {
                    "type": "string",
                    "description": "Exact service name, as returned by list_services."
                }
            },
            "required": ["service"],
            "additionalProperties": false
        })
    }
    fn call<'a>(
        &'a self,
        arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        // A tool's error goes back to the model, so it names the way out.
        let result = match arguments.get("service").and_then(Value::as_str) {
            None => Err(ToolError::Failed(
                "'service_status' needs a string argument 'service'".to_owned(),
            )),
            Some(wanted) => INVENTORY
                .iter()
                .find(|s| s.name == wanted)
                .map(|s| {
                    json!({
                        "service": s.name,
                        "state": s.state,
                        "version": s.version,
                        "uptimeSeconds": s.uptime_s,
                    })
                    .to_string()
                })
                .ok_or_else(|| {
                    ToolError::Failed(format!(
                        "no service named '{wanted}'; call list_services for the inventory"
                    ))
                }),
        };
        Box::pin(async move { result })
    }
}

/// Both tools, as the loop offers them.
///
/// # Errors
///
/// Never in practice; [`ToolSet`] refuses two tools of one name, and these
/// have two.
pub fn catalogue() -> Result<ToolSet, DuplicateTool> {
    ToolSet::new().with(ListServices)?.with(ServiceStatus)
}
