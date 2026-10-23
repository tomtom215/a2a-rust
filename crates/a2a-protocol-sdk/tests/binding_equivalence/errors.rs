// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `BIND-EQUIV-003` (spec §5.1): "all supported protocols MUST map errors
//! consistently using appropriate protocol-specific codes."
//!
//! Each fault is provoked over all four bindings. Consistently: this SDK's
//! client, decoding each binding's own wire form, recovers the same A2A
//! error on every one. Protocol-specific: on the two HTTP bindings the wire
//! itself is read, and carries §5.4's code for that binding — the JSON-RPC
//! error code, and the HTTP status with the `ErrorInfo` reason. (gRPC's
//! status codes are pinned against the same table in the server's
//! `dispatch/grpc/helpers_tests.rs`.)
//!
//! `CARD-EXT-002` is here too: a card declaring `extendedAgentCard` with no
//! extended card configured answers `ExtendedAgentCardNotConfiguredError`
//! on every binding.

use a2a_protocol_client::A2aClient;
use a2a_protocol_client::error::ClientError;
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};
use serde_json::json;

use crate::functionality::{GUARD, send};
use crate::harness::{Binding, raw, serve};

/// The A2A error a call was refused with, or why it was not one.
fn a2a_code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> Result<ErrorCode, String> {
    match result {
        Err(ClientError::Protocol(e)) => Ok(e.code),
        other => Err(format!("not an A2A error: {other:?}")),
    }
}

/// `SubscribeToTask`'s refusal: the call's error, or, where the binding
/// opens the stream first (WebSocket), its first item.
async fn subscribe_refusal(client: &A2aClient, id: &str) -> Result<ErrorCode, String> {
    match client.subscribe_to_task(id).await {
        Err(e) => a2a_code::<()>(Err(e)),
        Ok(mut stream) => match tokio::time::timeout(GUARD, stream.next()).await {
            Ok(Some(Err(e))) => a2a_code::<()>(Err(e)),
            other => Err(format!("the subscription was not refused: {other:?}")),
        },
    }
}

/// Every fault, as the client on `binding` decodes it.
async fn faults(client: &A2aClient, done: &str) -> Vec<(&'static str, Result<ErrorCode, String>)> {
    let continuation = MessageSendParams::new(
        Message::user_text(crate::functionality::uuid_like(), "more").with_task_id(done),
    );
    let unsupported_part = MessageSendParams::new(Message::user(
        crate::functionality::uuid_like(),
        vec![Part::raw("iVBORw0KGgo=").with_media_type("image/png")],
    ));
    vec![
        (
            "GetTask, unknown id",
            a2a_code(client.get_task(TaskQueryParams::new("no-such-task")).await),
        ),
        (
            "CancelTask, finished task",
            a2a_code(client.cancel_task(done).await),
        ),
        (
            "SubscribeToTask, finished task",
            subscribe_refusal(client, done).await,
        ),
        (
            "GetTaskPushNotificationConfig, unknown config",
            a2a_code(client.get_push_config(done, "no-such-config").await),
        ),
        (
            "SendMessage, continuing a finished task",
            a2a_code(client.send_message(continuation).await),
        ),
        (
            "SendMessage, a part the agent does not accept",
            a2a_code(client.send_message(unsupported_part).await),
        ),
    ]
}

#[tokio::test]
async fn bind_equiv_003_every_binding_decodes_the_same_a2a_error() {
    let agent = serve(false, true).await;
    let done = send(&agent.client(Binding::JsonRpc, None).await, "finished")
        .await
        .expect("send")
        .id
        .0;
    let expected = [
        ErrorCode::TaskNotFound,
        ErrorCode::TaskNotCancelable,
        ErrorCode::UnsupportedOperation,
        ErrorCode::TaskNotFound,
        ErrorCode::UnsupportedOperation,
        ErrorCode::ContentTypeNotSupported,
    ];
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        for ((fault, got), want) in faults(&client, &done).await.into_iter().zip(expected) {
            assert_eq!(got, Ok(want), "{binding:?}, {fault}");
        }
    }
}

/// Spec §3.3.2: an error carries a machine-readable code and a
/// human-readable message, whatever the binding. Every fault above, decoded
/// on every binding, has both: this SDK's clients pass each binding's
/// message through (JSON-RPC `error.message`, HTTP+JSON `error.message`,
/// the gRPC status message, the WebSocket frame's `error.message`), so an
/// empty one here is an empty one on the wire. The two HTTP bindings are
/// also read raw.
#[tokio::test]
async fn every_binding_s_error_carries_a_code_and_a_message() {
    let agent = serve(false, true).await;
    let done = send(&agent.client(Binding::JsonRpc, None).await, "finished")
        .await
        .expect("send")
        .id
        .0;
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        let unknown = client.get_task(TaskQueryParams::new("no-such-task")).await;
        let not_cancelable = client.cancel_task(&done).await;
        for (fault, result) in [
            ("GetTask, unknown id", unknown.map(drop)),
            ("CancelTask, finished task", not_cancelable.map(drop)),
        ] {
            match result {
                Err(ClientError::Protocol(e)) => {
                    assert!(!e.message.trim().is_empty(), "{binding:?}, {fault}: {e:?}")
                }
                other => panic!("{binding:?}, {fault}: not an A2A error: {other:?}"),
            }
        }
    }

    let rpc = json!({"jsonrpc": "2.0", "id": 1, "method": "GetTask", "params": {"id": "nope"}});
    let (_, answer) = raw(agent.url(Binding::JsonRpc), "POST", Some(rpc), Some("1.0")).await;
    assert_eq!(
        answer["error"]["code"],
        ErrorCode::TaskNotFound.as_i32(),
        "{answer}"
    );
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|m| !m.trim().is_empty()),
        "JSON-RPC: {answer}"
    );
    let url = format!("{}/tasks/nope", agent.url(Binding::HttpJson));
    let (status, answer) = raw(url, "GET", None, Some("1.0")).await;
    assert_eq!(status, 404, "{answer}");
    assert_eq!(answer["error"]["status"], "NOT_FOUND", "{answer}");
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|m| !m.trim().is_empty()),
        "HTTP+JSON: {answer}"
    );
}

/// The protocol-specific half, read off the wire of the two HTTP bindings:
/// JSON-RPC carries the A2A error code; HTTP+JSON carries §5.4's status and
/// the error's `ErrorInfo` reason.
#[tokio::test]
async fn bind_equiv_003_each_http_binding_uses_its_own_protocol_codes() {
    let agent = serve(false, true).await;
    let done = send(&agent.client(Binding::JsonRpc, None).await, "finished")
        .await
        .expect("send")
        .id
        .0;
    let cases = [
        (
            "GetTask",
            "no-such-task",
            "GET",
            "",
            ErrorCode::TaskNotFound,
        ),
        (
            "CancelTask",
            done.as_str(),
            "POST",
            ":cancel",
            ErrorCode::TaskNotCancelable,
        ),
    ];
    for (method, id, verb, suffix, code) in cases {
        let rpc = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": {"id": id}});
        let (_, answer) = raw(agent.url(Binding::JsonRpc), "POST", Some(rpc), Some("1.0")).await;
        assert_eq!(
            answer["error"]["code"].as_i64(),
            Some(i64::from(code.as_i32())),
            "JSON-RPC {method}: {answer}"
        );

        let body = (verb == "POST").then(|| json!({}));
        let url = format!("{}/tasks/{id}{suffix}", agent.url(Binding::HttpJson));
        let (status, answer) = raw(url, verb, body, Some("1.0")).await;
        assert_eq!(status, code.http_status(), "HTTP+JSON {method}: {answer}");
        let reasons: Vec<&str> = answer["error"]["details"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d["reason"].as_str())
            .collect();
        assert_eq!(
            reasons,
            [code.a2a_reason().expect("an A2A error has a reason")],
            "HTTP+JSON {method}: {answer}"
        );
    }
}

/// `CARD-EXT-002` (spec §3.1.11): "If the agent declares support but has
/// not configured an extended card, it MUST return
/// `ExtendedAgentCardNotConfiguredError`" — on every binding.
#[tokio::test]
async fn card_ext_002_a_declared_but_unconfigured_extended_card_is_refused_on_every_binding() {
    let agent = serve(false, false).await;
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        assert_eq!(
            a2a_code(client.get_extended_agent_card().await),
            Ok(ErrorCode::ExtendedAgentCardNotConfigured),
            "{binding:?}"
        );
    }
    let rpc = json!({"jsonrpc": "2.0", "id": 1, "method": "GetExtendedAgentCard", "params": {}});
    let (_, answer) = raw(agent.url(Binding::JsonRpc), "POST", Some(rpc), Some("1.0")).await;
    assert_eq!(answer["error"]["code"], -32007, "{answer}");
}
