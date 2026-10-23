// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §3.3.4 / §4.6.3: a client that does not declare an extension the
//! card marks `required` MUST be refused with
//! `ExtensionSupportRequiredError`. Every operation calls
//! `ensure_required_extensions`; until 2026-10-23 only `GetTask` and
//! `GetExtendedAgentCard` were tested, so dropping the call from any of the
//! other nine failed nothing.
//!
//! Each operation in `Method::ALL` is called without the declaration and
//! must be refused; then with it, and must not be. Through the handler, and
//! through both HTTP bindings, which reach it with the header they parsed.

use std::collections::HashMap;
use std::sync::Arc;

use a2a_protocol_server::agent_executor;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{EventEmitter, HttpPushSender, RequestHandler, RequestHandlerBuilder};
use a2a_protocol_types::error::{A2aError, ErrorCode};
use a2a_protocol_types::method::Method;
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams, TaskQueryParams};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::{Message, TaskState};
use serde_json::{Value, json};

use crate::{Answer, card, exchange, first_sse_data, headers, jsonrpc};

const REQUIRED: &str = "https://example.com/ext/required/v1";
const HOOK: &str = "https://hook.example.com/a2a";

struct Done;
agent_executor!(Done, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    emit.status(TaskState::Completed).await
});

fn handler() -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Done)
            .with_agent_card(card(&[(REQUIRED, true)]))
            .with_extended_agent_card(card(&[(REQUIRED, true)]))
            .allow_unauthenticated_extended_card()
            .with_push_sender(HttpPushSender::new())
            .build()
            .expect("handler"),
    )
}

fn declared() -> HashMap<String, String> {
    headers(&[("A2A-Extensions", REQUIRED)])
}

fn send_params(id: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user_text(id, "hi"))
}

/// A task to name, made by a caller that declares the extension, and a
/// push config on it.
async fn seed(handler: &RequestHandler) -> (String, String) {
    let task = match handler
        .on_send_message(send_params("seed"), false, Some(&declared()))
        .await
        .expect("seed send")
    {
        a2a_protocol_server::handler::SendMessageResult::Response(
            a2a_protocol_types::responses::SendMessageResponse::Task(t),
        ) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    };
    let config = handler
        .on_set_push_config(
            TaskPushNotificationConfig::new(task.as_str(), HOOK),
            Some(&declared()),
        )
        .await
        .expect("seed config");
    (task, config.id.expect("config id"))
}

/// Calls `method` on the handler, answering the A2A error it refused with.
pub async fn call(
    handler: &RequestHandler,
    method: Method,
    h: Option<&HashMap<String, String>>,
    task: &str,
    config: &str,
    n: usize,
) -> Result<(), A2aError> {
    let ids = |extra: Value| {
        let mut v = json!({"taskId": task, "id": config});
        if let (Some(v), Some(extra)) = (v.as_object_mut(), extra.as_object()) {
            v.extend(extra.clone());
        }
        v
    };
    let result = match method {
        Method::SendMessage => handler
            .on_send_message(send_params(&format!("m-{n}")), false, h)
            .await
            .map(drop),
        Method::SendStreamingMessage => handler
            .on_send_message(send_params(&format!("m-{n}")), true, h)
            .await
            .map(drop),
        Method::GetTask => handler
            .on_get_task(TaskQueryParams::new(task), h)
            .await
            .map(drop),
        Method::ListTasks => handler
            .on_list_tasks(ListTasksParams::default(), h)
            .await
            .map(drop),
        Method::CancelTask => handler
            .on_cancel_task(serde_json::from_value(json!({"id": task})).unwrap(), h)
            .await
            .map(drop),
        Method::SubscribeToTask => handler
            .on_resubscribe(serde_json::from_value(json!({"id": task})).unwrap(), h)
            .await
            .map(drop),
        Method::CreateTaskPushNotificationConfig => handler
            .on_set_push_config(TaskPushNotificationConfig::new(task, HOOK), h)
            .await
            .map(drop),
        Method::GetTaskPushNotificationConfig => handler
            .on_get_push_config(serde_json::from_value(ids(json!({}))).unwrap(), h)
            .await
            .map(drop),
        Method::ListTaskPushNotificationConfigs => {
            handler.on_list_push_configs(task, None, h).await.map(drop)
        }
        Method::DeleteTaskPushNotificationConfig => handler
            .on_delete_push_config(serde_json::from_value(ids(json!({}))).unwrap(), h)
            .await
            .map(drop),
        Method::GetExtendedAgentCard => handler.on_get_extended_agent_card(h).await.map(drop),
        other => panic!("{other:?} is in Method::ALL but not in this table; add it"),
    };
    result.map_err(|e| e.to_a2a_error())
}

#[tokio::test]
async fn every_operation_refuses_a_client_that_does_not_declare_a_required_extension() {
    let handler = handler();
    let (task, config) = seed(&handler).await;
    // Undeclared: no header, and a header naming only other extensions.
    let other = headers(&[("A2A-Extensions", "https://example.com/ext/other/v1")]);
    for (n, method) in Method::ALL.iter().copied().enumerate() {
        for h in [None, Some(&other)] {
            let err = call(&handler, method, h, &task, &config, n)
                .await
                .expect_err(&format!("{method:?} with {h:?} was not refused"));
            assert_eq!(
                err.code,
                ErrorCode::ExtensionSupportRequired,
                "{method:?}: {err:?}"
            );
            assert!(
                err.message.contains(REQUIRED),
                "{method:?} names it: {err:?}"
            );
        }
    }
}

/// The control: with the declaration, no operation answers
/// `ExtensionSupportRequired`, and the ones that can succeed here do.
#[tokio::test]
async fn every_operation_admits_a_client_that_declares_the_required_extension() {
    let handler = handler();
    let (task, config) = seed(&handler).await;
    // Delete last: the earlier calls name the config it removes.
    for (n, method) in Method::ALL.iter().copied().enumerate() {
        let result = call(&handler, method, Some(&declared()), &task, &config, n).await;
        match method {
            // The seeded task is finished: cancel and subscribe are refused
            // for that, not for the extension.
            Method::CancelTask | Method::SubscribeToTask => {
                let err = result.expect_err("a finished task");
                assert_ne!(err.code, ErrorCode::ExtensionSupportRequired, "{method:?}");
            }
            _ => assert!(result.is_ok(), "{method:?}: {result:?}"),
        }
    }
}

/// The JSON-RPC params and the HTTP+JSON request for `method`.
pub fn route(
    method: Method,
    task: &str,
    config: &str,
) -> (Value, &'static str, String, Option<Value>) {
    let message =
        json!({"message": {"messageId": "m-wire", "role": "ROLE_USER", "parts": [{"text": "hi"}]}});
    let push = json!({"taskId": task, "url": HOOK});
    match method {
        Method::SendMessage => (
            message.clone(),
            "POST",
            "/message:send".into(),
            Some(message),
        ),
        Method::SendStreamingMessage => (
            message.clone(),
            "POST",
            "/message:stream".into(),
            Some(message),
        ),
        Method::GetTask => (json!({"id": task}), "GET", format!("/tasks/{task}"), None),
        Method::ListTasks => (json!({}), "GET", "/tasks".into(), None),
        Method::CancelTask => (
            json!({"id": task}),
            "POST",
            format!("/tasks/{task}:cancel"),
            Some(json!({})),
        ),
        Method::SubscribeToTask => (
            json!({"id": task}),
            "POST",
            format!("/tasks/{task}:subscribe"),
            Some(json!({})),
        ),
        Method::CreateTaskPushNotificationConfig => (
            push.clone(),
            "POST",
            format!("/tasks/{task}/pushNotificationConfigs"),
            Some(push),
        ),
        Method::GetTaskPushNotificationConfig => (
            json!({"taskId": task, "id": config}),
            "GET",
            format!("/tasks/{task}/pushNotificationConfigs/{config}"),
            None,
        ),
        Method::ListTaskPushNotificationConfigs => (
            json!({"taskId": task}),
            "GET",
            format!("/tasks/{task}/pushNotificationConfigs"),
            None,
        ),
        Method::DeleteTaskPushNotificationConfig => (
            json!({"taskId": task, "id": config}),
            "DELETE",
            format!("/tasks/{task}/pushNotificationConfigs/{config}"),
            None,
        ),
        Method::GetExtendedAgentCard => (json!({}), "GET", "/extendedAgentCard".into(), None),
        other => panic!("{other:?} is in Method::ALL but has no route here; add one"),
    }
}

/// The error object of a JSON or SSE answer, or `None` when it carries none.
fn error_of(answer: &Answer) -> Option<Value> {
    let body = if answer.body.trim_start().starts_with('{') {
        answer.json()
    } else if answer.body.is_empty() {
        return None;
    } else {
        first_sse_data(&answer.body)
    };
    body.get("error").cloned()
}

fn reason(error: &Value) -> Option<String> {
    let details = error.get("details").or_else(|| error.get("data"))?;
    details
        .as_array()?
        .iter()
        .find_map(|d| d["reason"].as_str().map(str::to_owned))
}

#[tokio::test]
async fn both_http_bindings_refuse_every_operation_without_the_declaration() {
    let handler = handler();
    let (task, config) = seed(&handler).await;
    let rpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("JSON-RPC");
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("HTTP+JSON");
    let code = ErrorCode::ExtensionSupportRequired;
    for method in Method::ALL.iter().copied() {
        let (params, verb, path, body) = route(method, &task, &config);
        for extra in [&[][..], &[("A2A-Extensions", REQUIRED)][..]] {
            let declared = !extra.is_empty();

            let answer = jsonrpc(rpc, method.wire_name(), &params, extra).await;
            let error = error_of(&answer);
            let refused = error
                .as_ref()
                .is_some_and(|e| e["code"].as_i64() == Some(i64::from(code.as_i32())));
            assert_eq!(
                refused, !declared,
                "JSON-RPC {method:?}, declared {declared}: {answer:?}"
            );

            let answer = exchange(rest, verb, &path, body.as_ref(), extra).await;
            let refused = answer.status == code.http_status()
                && error_of(&answer).as_ref().and_then(reason).as_deref() == code.a2a_reason();
            assert_eq!(
                refused, !declared,
                "HTTP+JSON {method:?}, declared {declared}: {answer:?}"
            );
        }
    }
}
