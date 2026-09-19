// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The shipped `mcp-tool-server`, spawned as a real child process.
//!
//! `src/tests.rs` drives the bridge over an in-process pipe against a
//! purpose-built server. That covers the branches but proves nothing about
//! the binary this example actually ships, or about the spawn: a server whose
//! `main` wrote a banner to stdout would corrupt the very first frame and
//! every in-process test would still pass. This file is the check that the
//! thing in `target/` works when started the way a real MCP client starts
//! one.
//!
//! `CARGO_BIN_EXE_mcp-tool-server` is set by cargo for integration tests, so
//! the path is the built binary rather than a guess about where it lives.

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService, ServiceError};
use rmcp::transport::TokioChildProcess;
use serde_json::json;

/// Spawns the shipped server and completes the MCP handshake.
async fn spawn_server() -> RunningService<RoleClient, ()> {
    let command = tokio::process::Command::new(env!("CARGO_BIN_EXE_mcp-tool-server"));
    let transport = TokioChildProcess::new(command).expect("the built server binary spawns");
    ().serve(transport)
        .await
        .expect("the server completes the MCP handshake over stdio")
}

/// Text content of a successful call, joined in order.
fn text_of(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            rmcp::model::ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn the_shipped_server_publishes_both_tools_with_usable_schemas() {
    let service = spawn_server().await;
    let tools = service.list_all_tools().await.expect("tools/list answers");

    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(names, ["list_services", "service_status"]);

    let status = tools
        .iter()
        .find(|t| t.name == "service_status")
        .expect("service_status is published");
    // The schema is derived from `ServiceStatusArgs`, so this asserts the
    // derive is actually reaching the wire — the failure mode being a struct
    // change that never shows up in what the model is told.
    assert_eq!(
        status.input_schema.get("required"),
        Some(&json!(["service"])),
        "{:?}",
        status.input_schema
    );
    assert!(
        status
            .description
            .as_deref()
            .is_some_and(|d| d.contains("uptime")),
        "the doc comment is the description: {:?}",
        status.description
    );

    let list = tools
        .iter()
        .find(|t| t.name == "list_services")
        .expect("list_services is published");
    // A no-argument tool still needs an object schema; providers reject any
    // other top-level type for function parameters.
    assert_eq!(list.input_schema.get("type"), Some(&json!("object")));
}

#[tokio::test]
async fn the_shipped_server_answers_a_real_lookup() {
    let service = spawn_server().await;
    let result = service
        .call_tool(
            CallToolRequestParams::new("service_status").with_arguments(
                json!({ "service": "checkout" })
                    .as_object()
                    .expect("an object literal")
                    .clone(),
            ),
        )
        .await
        .expect("checkout is in the inventory");

    let row: serde_json::Value =
        serde_json::from_str(&text_of(&result)).expect("the tool returns JSON text");
    assert_eq!(row["service"], "checkout");
    assert_eq!(row["state"], "healthy");
    assert_eq!(row["version"], "2.8.0");
    assert_eq!(row["uptimeSeconds"], 259_200);
}

#[tokio::test]
async fn the_shipped_server_refuses_an_unknown_service_over_the_wire() {
    let service = spawn_server().await;
    let refused = service
        .call_tool(
            CallToolRequestParams::new("service_status").with_arguments(
                json!({ "service": "billing" })
                    .as_object()
                    .expect("an object literal")
                    .clone(),
            ),
        )
        .await
        .expect_err("billing is not in the inventory");

    // A tool-level refusal, not a broken session. `crate::mcp` depends on
    // this being `McpError` and not a transport variant, and the distinction
    // is only observable against a real server.
    match refused {
        ServiceError::McpError(data) => {
            assert!(
                data.message.contains("no service named 'billing'"),
                "{data:?}"
            );
            assert!(
                data.message.contains("list_services"),
                "the refusal should name the recovery tool: {data:?}"
            );
        }
        other => panic!("expected a tool-level refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn the_shipped_server_writes_nothing_but_mcp_to_stdout() {
    // The failure this catches is a `println!` added to the server's `main`:
    // stdout is the wire, so one stray line desynchronizes every frame after
    // it. A handshake that completes and a call that answers is the proof —
    // both would break on the first junk byte.
    let service = spawn_server().await;
    let result = service
        .call_tool(CallToolRequestParams::new("list_services"))
        .await
        .expect("list_services takes no arguments");

    let names: Vec<String> =
        serde_json::from_str(&text_of(&result)).expect("the tool returns a JSON array");
    assert_eq!(names, ["payments-api", "checkout", "search"]);
}
