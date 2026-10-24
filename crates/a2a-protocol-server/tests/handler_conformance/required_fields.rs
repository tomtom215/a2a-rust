// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §5.7: a field a2a.proto marks REQUIRED "MUST be present and set";
//! implementations "SHOULD validate these requirements and reject messages
//! with missing required fields". §11.6 / §9.5: the rejection is
//! `InvalidParams` with a `google.rpc.BadRequest` naming the field.
//!
//! One row per REQUIRED field of the inbound requests, each sent absent and
//! empty where both are expressible, through JSON-RPC and HTTP+JSON. Each
//! must be `-32602` / `400 INVALID_ARGUMENT` with the field's wire path in
//! the violation. Three kinds of row failed before 2026-10-23: a missing key
//! was named by its parent (`message`, or nothing), the send ids by their
//! Rust names (`message.id`, `task_id`), and an empty push-config `url`
//! answered `TaskNotFound` for an unknown task.
//!
//! The executor's events are held to the same fields by
//! `tests/executor_event_fields.rs`, which writes each broken event kind.

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{HttpPushSender, RequestHandlerBuilder, agent_executor};
use serde_json::{Value, json};

use crate::{Answer, card, exchange, jsonrpc};

const BAD_REQUEST: &str = "type.googleapis.com/google.rpc.BadRequest";
const HOOK: &str = "https://hook.example.com/a2a";

struct Exec;
agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

async fn serve() -> (SocketAddr, SocketAddr) {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Exec)
            .with_agent_card(card(&[]))
            .with_push_sender(HttpPushSender::new())
            .build()
            .expect("handler"),
    );
    let rpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("JSON-RPC");
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler))
        .await
        .expect("HTTP+JSON");
    (rpc, rest)
}

/// A message with `edit` applied: a key set, or removed when `Null`.
fn message(edit: &[(&str, Value)]) -> Value {
    let mut m = json!({"messageId": "m-1", "role": "ROLE_USER", "parts": [{"text": "hi"}]});
    for (key, value) in edit {
        if value.is_null() {
            m.as_object_mut().expect("object").remove(*key);
        } else {
            m[*key] = value.clone();
        }
    }
    json!({ "message": m })
}

/// The one field the `BadRequest` detail names.
fn violation(details: &Value, what: &str) -> String {
    let detail = details
        .as_array()
        .and_then(|d| d.iter().find(|d| d["@type"] == BAD_REQUEST))
        .unwrap_or_else(|| panic!("{what}: no BadRequest detail in {details}"));
    let violation = &detail["fieldViolations"][0];
    assert!(
        violation["description"]
            .as_str()
            .is_some_and(|d| !d.is_empty()),
        "{what}: {detail}"
    );
    violation["field"].as_str().unwrap_or_default().to_owned()
}

fn rpc_field(answer: &Answer, what: &str) -> String {
    let body = answer.json();
    assert_eq!(body["error"]["code"], -32602, "{what}: {body}");
    violation(&body["error"]["data"], what)
}

fn rest_field(answer: &Answer, what: &str) -> String {
    let body = answer.json();
    assert_eq!(answer.status, 400, "{what}: {body}");
    assert_eq!(
        body["error"]["status"], "INVALID_ARGUMENT",
        "{what}: {body}"
    );
    violation(&body["error"]["details"], what)
}

/// `SendMessage`: the same body is the JSON-RPC params and the HTTP+JSON
/// request.
#[tokio::test]
async fn a_send_missing_a_required_field_is_invalid_params_naming_it_on_both_bindings() {
    let (rpc, rest) = serve().await;
    let rows: Vec<(&str, Value, &str)> = vec![
        (
            "role absent",
            message(&[("role", Value::Null)]),
            "message.role",
        ),
        (
            "role unspecified",
            message(&[("role", json!("ROLE_UNSPECIFIED"))]),
            "message.role",
        ),
        (
            "messageId absent",
            message(&[("messageId", Value::Null)]),
            "message.messageId",
        ),
        (
            "messageId empty",
            message(&[("messageId", json!(""))]),
            "message.messageId",
        ),
        (
            "parts absent",
            message(&[("parts", Value::Null)]),
            "message.parts",
        ),
        (
            "parts empty",
            message(&[("parts", json!([]))]),
            "message.parts",
        ),
        (
            "part content absent",
            message(&[("parts", json!([{}]))]),
            "message.parts[0]",
        ),
        ("message absent", json!({}), "message"),
        (
            "taskId blank",
            message(&[("taskId", json!(" "))]),
            "message.taskId",
        ),
        (
            "contextId blank",
            message(&[("contextId", json!(" "))]),
            "message.contextId",
        ),
        (
            "referenceTaskIds entry blank",
            message(&[("referenceTaskIds", json!([" "]))]),
            "message.referenceTaskIds",
        ),
    ];
    for (what, body, field) in rows {
        let answer = jsonrpc(rpc, "SendMessage", &body, &[]).await;
        assert_eq!(rpc_field(&answer, what), field, "JSON-RPC, {what}");
        let answer = exchange(rest, "POST", "/message:send", Some(&body), &[]).await;
        assert_eq!(rest_field(&answer, what), field, "HTTP+JSON, {what}");
    }
}

/// The task and push-config operations. On HTTP+JSON the ids are in the
/// path, so only the rows a path can express are sent there.
#[tokio::test]
async fn a_task_or_push_config_request_missing_a_required_field_is_invalid_params_naming_it() {
    let (rpc, rest) = serve().await;
    let create = "CreateTaskPushNotificationConfig";
    let rpc_rows: Vec<(&str, Value, &str)> = vec![
        ("GetTask", json!({}), "id"),
        ("GetTask", json!({"id": ""}), "id"),
        ("CancelTask", json!({"id": ""}), "id"),
        ("SubscribeToTask", json!({"id": ""}), "id"),
        (create, json!({"taskId": "t"}), "url"),
        (create, json!({"taskId": "t", "url": ""}), "url"),
        (create, json!({"url": HOOK}), "taskId"),
        (create, json!({"taskId": "", "url": HOOK}), "taskId"),
        (
            "GetTaskPushNotificationConfig",
            json!({"taskId": "t", "id": ""}),
            "id",
        ),
        (
            "GetTaskPushNotificationConfig",
            json!({"taskId": "", "id": "c"}),
            "taskId",
        ),
        (
            "ListTaskPushNotificationConfigs",
            json!({"taskId": ""}),
            "taskId",
        ),
        (
            "DeleteTaskPushNotificationConfig",
            json!({"taskId": "t", "id": ""}),
            "id",
        ),
        (
            "DeleteTaskPushNotificationConfig",
            json!({"taskId": "", "id": "c"}),
            "taskId",
        ),
    ];
    for (method, params, field) in rpc_rows {
        let what = format!("{method} {params}");
        let answer = jsonrpc(rpc, method, &params, &[]).await;
        assert_eq!(rpc_field(&answer, &what), field, "JSON-RPC, {what}");
    }

    let configs = "/tasks/t/pushNotificationConfigs";
    let rest_rows: Vec<(&str, &str, Option<Value>, &str)> = vec![
        ("GET", "/tasks/%20", None, "id"),
        ("POST", "/tasks/%20:cancel", Some(json!({})), "id"),
        ("POST", configs, Some(json!({"taskId": "t"})), "url"),
        (
            "POST",
            configs,
            Some(json!({"taskId": "t", "url": ""})),
            "url",
        ),
    ];
    for (verb, path, body, field) in rest_rows {
        let what = format!("{verb} {path} {body:?}");
        let answer = exchange(rest, verb, path, body.as_ref(), &[]).await;
        assert_eq!(rest_field(&answer, &what), field, "HTTP+JSON, {what}");
    }
}
