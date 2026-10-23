// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §3.3.2: a server answering a task the caller may not see "MUST NOT
//! reveal the existence of resources the client is not authorized to
//! access", which is done by answering it as it answers a task that does
//! not exist. `caller_isolation.rs` shows the handler returns
//! `TaskNotFound` for both; this compares what reaches the wire, status,
//! headers and body, with the id substituted, on both HTTP bindings and for
//! every operation that names a task.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{
    BearerTokenAuthInterceptor, HttpPushSender, RequestHandler, RequestHandlerBuilder,
};
use a2a_protocol_types::responses::SendMessageResponse;
use serde_json::{Value, json};

use crate::s3_agent::{Scripted, Seen, immediately, send};
use crate::{Answer, exchange, headers, jsonrpc};

const HOOK: &str = "https://hook.example.com/a2a";
/// As long as a minted task id, so even `content-length` matches.
const UNKNOWN: &str = "00000000-0000-4000-8000-000000000000";

fn handler() -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Scripted(Arc::new(Seen::default())))
            .without_agent_card()
            .with_push_sender(HttpPushSender::new())
            .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
                ("alice-token", "alice"),
                ("bob-token", "bob"),
            ]))
            .isolate_tasks_by_caller()
            .build()
            .expect("handler"),
    )
}

/// Alice's task, still running (`wait` holds it at `WORKING`), with a push
/// config on it.
async fn alices_task(h: &RequestHandler) -> (String, String) {
    let alice = headers(&[("Authorization", "Bearer alice-token")]);
    let id = match h
        .on_send_message(
            immediately(send("m-a", "wait", None, None)),
            false,
            Some(&alice),
        )
        .await
        .expect("alice sends")
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    };
    let config = h
        .on_set_push_config(
            a2a_protocol_types::push::TaskPushNotificationConfig::new(id.as_str(), HOOK),
            Some(&alice),
        )
        .await
        .expect("alice's config");
    (id, config.id.expect("config id"))
}

/// The answer with what legitimately differs between two calls removed:
/// the id asked about, and the `date` header.
fn comparable(answer: &Answer, id: &str) -> (u16, BTreeMap<String, String>, String) {
    let headers = answer
        .headers
        .iter()
        .filter(|(k, _)| k.as_str() != "date")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    (answer.status, headers, answer.body.replace(id, "{id}"))
}

/// Each operation that names a task: the JSON-RPC call and the HTTP+JSON
/// request, for task `id` and push config `config`.
fn calls(
    id: &str,
    config: &str,
) -> Vec<(&'static str, Value, &'static str, String, Option<Value>)> {
    let message = json!({"message": {
        "messageId": "m-b", "role": "ROLE_USER", "parts": [{"text": "more"}], "taskId": id
    }});
    let push = json!({"taskId": id, "url": HOOK});
    vec![
        (
            "GetTask",
            json!({"id": id}),
            "GET",
            format!("/tasks/{id}"),
            None,
        ),
        (
            "CancelTask",
            json!({"id": id}),
            "POST",
            format!("/tasks/{id}:cancel"),
            Some(json!({})),
        ),
        (
            "SubscribeToTask",
            json!({"id": id}),
            "POST",
            format!("/tasks/{id}:subscribe"),
            Some(json!({})),
        ),
        (
            "SendMessage",
            message.clone(),
            "POST",
            "/message:send".into(),
            Some(message),
        ),
        (
            "CreateTaskPushNotificationConfig",
            push.clone(),
            "POST",
            format!("/tasks/{id}/pushNotificationConfigs"),
            Some(push),
        ),
        (
            "GetTaskPushNotificationConfig",
            json!({"taskId": id, "id": config}),
            "GET",
            format!("/tasks/{id}/pushNotificationConfigs/{config}"),
            None,
        ),
        (
            "ListTaskPushNotificationConfigs",
            json!({"taskId": id}),
            "GET",
            format!("/tasks/{id}/pushNotificationConfigs"),
            None,
        ),
        (
            "DeleteTaskPushNotificationConfig",
            json!({"taskId": id, "id": config}),
            "DELETE",
            format!("/tasks/{id}/pushNotificationConfigs/{config}"),
            None,
        ),
    ]
}

async fn answers(
    rpc: SocketAddr,
    rest: SocketAddr,
    id: &str,
    config: &str,
) -> Vec<(String, (u16, BTreeMap<String, String>, String))> {
    let bob = [("Authorization", "Bearer bob-token")];
    let mut out = Vec::new();
    for (method, params, verb, path, body) in calls(id, config) {
        let answer = jsonrpc(rpc, method, &params, &bob).await;
        out.push((format!("JSON-RPC {method}"), comparable(&answer, id)));
        let answer = exchange(rest, verb, &path, body.as_ref(), &bob).await;
        out.push((format!("HTTP+JSON {method}"), comparable(&answer, id)));
    }
    out
}

#[tokio::test]
async fn another_callers_task_and_an_unknown_one_answer_identically_on_the_wire() {
    let h = handler();
    let (id, config) = alices_task(&h).await;
    assert_eq!(id.len(), UNKNOWN.len(), "a minted id is a UUID");
    let rpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&h)))
        .await
        .expect("JSON-RPC");
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::clone(&h)))
        .await
        .expect("HTTP+JSON");

    let hidden = answers(rpc, rest, &id, &config).await;
    let unknown = answers(rpc, rest, UNKNOWN, &config).await;
    for ((what, got), (_, control)) in hidden.iter().zip(&unknown) {
        assert_eq!(got, control, "{what}: Bob's answer for Alice's task");
    }
    // Not vacuous: each answer is a not-found refusal.
    for (what, (status, _, body)) in &unknown {
        let not_found = if what.starts_with("JSON-RPC") {
            body.contains("\"code\":-32001")
        } else {
            *status == 404
        };
        assert!(not_found, "{what}: {status} {body}");
    }
    // And Alice's task was untouched by any of it.
    let alice = headers(&[("Authorization", "Bearer alice-token")]);
    let task = h
        .on_get_task(
            a2a_protocol_types::params::TaskQueryParams::new(&id),
            Some(&alice),
        )
        .await
        .expect("alice reads her task");
    assert!(!task.status.state.is_terminal(), "{task:?}");
}
