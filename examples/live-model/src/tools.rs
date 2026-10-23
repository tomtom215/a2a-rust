// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The worker's tools: `tool-loop-agent`'s inventory, read-only, and one tool
//! with a side effect, `restart_service`, which counts what it really did so
//! the idempotency check can tell a call the loop answered from an earlier
//! one from a call that ran.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_sdk::server::tool_loop::{
    DuplicateTool, Tool, ToolError, ToolInvocation, ToolSet,
};
use serde_json::{Value, json};

/// Each service: state, version, uptime in seconds.
const INVENTORY: &[(&str, &str, &str, u64)] = &[
    ("payments-api", "degraded", "4.2.1", 1_814_400),
    ("checkout", "healthy", "2.8.0", 259_200),
    ("search", "healthy", "1.19.3", 7_776_000),
];

type Fut<'a> = Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>>;

fn service_arg(invocation: &ToolInvocation, tool: &str) -> Result<String, ToolError> {
    let wanted = invocation
        .arguments
        .get("service")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Failed(format!("'{tool}' needs a string argument 'service'")))?;
    INVENTORY
        .iter()
        .find(|s| s.0 == wanted)
        .map(|s| s.0.to_owned())
        .ok_or_else(|| {
            ToolError::Failed(format!(
                "no service named '{wanted}'; call list_services for the inventory"
            ))
        })
}

fn service_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {"service": {"type": "string", "description": "Exact service name, as returned by list_services."}},
        "required": ["service"],
        "additionalProperties": false
    })
}

struct ListServices;

impl Tool for ListServices {
    fn name(&self) -> &'static str {
        "list_services"
    }
    fn description(&self) -> &'static str {
        "List the name of every service in the inventory. Call this first when the caller names a service you do not recognise."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn call<'a>(&'a self, _call: ToolInvocation) -> Fut<'a> {
        let names: Vec<&str> = INVENTORY.iter().map(|s| s.0).collect();
        Box::pin(async move { Ok(Value::from(names).to_string()) })
    }
}

struct ServiceStatus;

impl Tool for ServiceStatus {
    fn name(&self) -> &'static str {
        "service_status"
    }
    fn description(&self) -> &'static str {
        "Look up one service's current state, deployed version, and uptime in seconds."
    }
    fn parameters(&self) -> Value {
        service_parameters()
    }
    fn read_only(&self) -> bool {
        true
    }
    fn call<'a>(&'a self, invocation: ToolInvocation) -> Fut<'a> {
        let result = service_arg(&invocation, "service_status").map(|name| {
            let s = INVENTORY.iter().find(|s| s.0 == name).expect("checked");
            json!({"service": s.0, "state": s.1, "version": s.2, "uptimeSeconds": s.3}).to_string()
        });
        Box::pin(async move { result })
    }
}

/// What `restart_service` really did: the restarts per service, and the
/// idempotency key each one carried.
#[derive(Clone, Debug, Default)]
pub struct Restarts(Arc<Mutex<HashMap<String, Vec<String>>>>);

impl Restarts {
    /// How many times `service` was really restarted.
    pub fn count(&self, service: &str) -> usize {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(service)
            .map_or(0, Vec::len)
    }

    /// The idempotency keys the restarts of `service` carried.
    pub fn keys(&self, service: &str) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(service)
            .cloned()
            .unwrap_or_default()
    }
}

struct RestartService(Restarts);

impl Tool for RestartService {
    fn name(&self) -> &'static str {
        "restart_service"
    }
    fn description(&self) -> &'static str {
        "Restart one service. This changes the system: only call it when the caller asks for a restart."
    }
    fn parameters(&self) -> Value {
        service_parameters()
    }
    fn call<'a>(&'a self, invocation: ToolInvocation) -> Fut<'a> {
        let result = service_arg(&invocation, "restart_service").map(|name| {
            let mut done = self
                .0
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let keys = done.entry(name.clone()).or_default();
            keys.push(invocation.idempotency_key.clone());
            json!({"service": name, "restarted": true, "restartNumber": keys.len()}).to_string()
        });
        Box::pin(async move { result })
    }
}

/// The worker's tools, with `restarts` recording what `restart_service` did.
pub fn catalogue(restarts: &Restarts) -> Result<ToolSet, DuplicateTool> {
    ToolSet::new()
        .with(ListServices)?
        .with(ServiceStatus)?
        .with(RestartService(restarts.clone()))
}
