// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §14: implementations "MUST sanitize user-provided content". A
//! client's ids are stored, keyed on and logged — `GetTask` and `CancelTask`
//! log the requested id with `%`, so a `\r\n` in it wrote a line of the
//! client's choosing into the server's log. `validate_id` refused only empty
//! and over-long ids; the push-config ids and the `ListTasks` context filter
//! did not reach it at all. Every id is now refused when it holds a control
//! character, as the idempotency key already was, with the `InvalidParams`
//! an empty id gets: `-32602` on JSON-RPC, `400` on HTTP+JSON.
//!
//! Each refusal has a control: the same call with a non-ASCII id is served,
//! so it is the control character that is refused and not the alphabet.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::push::PushSender;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{RequestHandler, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use serde_json::{Value, json};

struct Exec;
agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

struct Noop;
impl PushSender for Noop {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

fn handler() -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Exec)
            .without_agent_card()
            .with_push_sender(Noop)
            .build()
            .expect("handler"),
    )
}

/// The ids every test sends: one with CR LF, and one of letters outside
/// ASCII that must still pass.
const INJECTED: &str = "a\r\nb";
const UNICODE: &str = "tâche-任务-задача";

async fn post(addr: std::net::SocketAddr, method: &str, path: &str, body: Value) -> (u16, Value) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let req = hyper::Request::builder()
        .method(method)
        .uri(format!("http://{addr}{path}"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .body(Full::new(Bytes::from(body.to_string())))
        .expect("request");
    let resp = client.request(req).await.expect("http");
    let status = resp.status().as_u16();
    let bytes = resp.collect().await.expect("body").to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn rpc(addr: std::net::SocketAddr, method: &str, params: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    post(addr, "POST", "/", body).await.1
}

fn send(message_id: &str, context_id: Option<&str>) -> Value {
    let mut message = json!({
        "messageId": message_id,
        "role": "ROLE_USER",
        "parts": [{"text": "hi"}],
    });
    if let Some(context) = context_id {
        message["contextId"] = json!(context);
    }
    json!({"message": message})
}

/// The `InvalidParams` a JSON-RPC answer carries, naming the field.
fn assert_refused(resp: &Value, what: &str) {
    assert_eq!(resp["error"]["code"], -32602, "{what}: {resp}");
}

#[tokio::test]
async fn jsonrpc_send_message_id_with_crlf_is_invalid_params() {
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler()))
        .await
        .unwrap();
    assert_refused(
        &rpc(addr, "SendMessage", send(INJECTED, None)).await,
        "messageId",
    );
    assert_refused(
        &rpc(addr, "SendMessage", send("m-1", Some(INJECTED))).await,
        "contextId",
    );
    // A control character the trim in `validate_id` used to strip before
    // anything was checked: the id stored is the untrimmed one.
    assert_refused(
        &rpc(addr, "SendMessage", send("m-2\n", None)).await,
        "messageId with a trailing LF",
    );

    let served = rpc(addr, "SendMessage", send(UNICODE, Some(UNICODE))).await;
    assert!(served.get("error").is_none(), "unicode ids: {served}");
    assert_eq!(served["result"]["task"]["contextId"], UNICODE, "{served}");
}

#[tokio::test]
async fn jsonrpc_task_id_with_crlf_is_invalid_params() {
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler()))
        .await
        .unwrap();
    for method in ["GetTask", "CancelTask", "SubscribeToTask"] {
        assert_refused(&rpc(addr, method, json!({"id": INJECTED})).await, method);
    }
    assert_refused(
        &rpc(addr, "ListTasks", json!({"contextId": INJECTED})).await,
        "ListTasks contextId",
    );
    // The control: a well-formed unknown id is not refused as malformed.
    let unknown = rpc(addr, "GetTask", json!({"id": UNICODE})).await;
    assert_eq!(unknown["error"]["code"], -32001, "TaskNotFound: {unknown}");
}

/// The push-config ids never reached `validate_id`: `taskId` and `id` were
/// checked only for being empty, and a created config's `id` not at all.
#[tokio::test]
async fn jsonrpc_push_config_ids_with_crlf_are_invalid_params() {
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler()))
        .await
        .unwrap();
    let task = rpc(addr, "SendMessage", send("m-1", None)).await;
    let task_id = task["result"]["task"]["id"].as_str().expect("task id");

    let create = |id: &str, task_id: &str| json!({"taskId": task_id, "id": id, "url": "https://example.com/hook"});
    assert_refused(
        &rpc(
            addr,
            "CreateTaskPushNotificationConfig",
            create(INJECTED, task_id),
        )
        .await,
        "Create id",
    );
    assert_refused(
        &rpc(
            addr,
            "CreateTaskPushNotificationConfig",
            create("cfg", INJECTED),
        )
        .await,
        "Create taskId",
    );
    for method in [
        "GetTaskPushNotificationConfig",
        "DeleteTaskPushNotificationConfig",
    ] {
        let by_id = rpc(addr, method, json!({"taskId": task_id, "id": INJECTED})).await;
        assert_refused(&by_id, method);
        let by_task = rpc(addr, method, json!({"taskId": INJECTED, "id": "cfg"})).await;
        assert_refused(&by_task, method);
    }
    assert_refused(
        &rpc(
            addr,
            "ListTaskPushNotificationConfigs",
            json!({"taskId": INJECTED}),
        )
        .await,
        "List taskId",
    );

    // An inline config's id is refused with the message's own, before the
    // task it would have been stored for is created.
    let tasks = |resp: Value| resp["result"]["tasks"].as_array().map(Vec::len);
    let before = tasks(rpc(addr, "ListTasks", json!({})).await);
    let mut inline = send("m-inline", None);
    inline["configuration"] = json!({
        "taskPushNotificationConfig": {"id": INJECTED, "url": "https://example.com/hook"}
    });
    assert_refused(
        &rpc(addr, "SendMessage", inline).await,
        "inline push config id",
    );
    let after = tasks(rpc(addr, "ListTasks", json!({})).await);
    assert_eq!(before, after, "the refused send created no task");

    let created = rpc(
        addr,
        "CreateTaskPushNotificationConfig",
        create(UNICODE, task_id),
    )
    .await;
    assert_eq!(created["result"]["id"], UNICODE, "{created}");
}

#[tokio::test]
async fn jsonrpc_tenant_with_control_character_is_invalid_params() {
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler()))
        .await
        .unwrap();
    for tenant in [INJECTED, "acme\u{1f}alice", "acme\u{85}"] {
        let resp = rpc(addr, "GetTask", json!({"id": "t", "tenant": tenant})).await;
        assert_refused(&resp, &format!("tenant {tenant:?}"));
    }
}

#[tokio::test]
async fn rest_message_and_task_ids_with_crlf_are_400() {
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler()))
        .await
        .unwrap();
    let (status, body) = post(addr, "POST", "/message:send", send(INJECTED, None)).await;
    assert_eq!(status, 400, "messageId: {body}");

    // `%0D%0A`: the dispatcher decodes path variables (spec §11.3).
    let (status, body) = post(addr, "GET", "/tasks/a%0D%0Ab", Value::Null).await;
    assert_eq!(status, 400, "task id in the path: {body}");
    let (status, body) = post(addr, "POST", "/tasks/a%0D%0Ab:cancel", json!({})).await;
    assert_eq!(status, 400, "cancel: {body}");

    let (status, body) = post(addr, "POST", "/message:send", send(UNICODE, None)).await;
    assert_eq!(status, 200, "unicode messageId: {body}");
}
