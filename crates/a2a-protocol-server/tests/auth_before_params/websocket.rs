// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! WebSocket: each frame is a call, admitted on its own with the credential
//! the upgrade carried.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use a2a_protocol_server::dispatch::WebSocketDispatcher;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;

use super::{CHALLENGE, Misses, TOKENS, gated, handler_refused_calls};

/// One frame per method whose params cannot be read, as in
/// `jsonrpc_auth_before_params.rs`, then the frames whose params the
/// handler refuses.
fn frames() -> Vec<Value> {
    let malformed = [
        ("SendMessage", json!({"message": "not a message"})),
        ("SendStreamingMessage", json!({"message": 7})),
        ("SubscribeToTask", json!({"id": 7})),
        ("GetTask", json!({"id": 7})),
        ("ListTasks", json!({"pageSize": "ten"})),
        ("CancelTask", json!({})),
        ("CreateTaskPushNotificationConfig", json!({"taskId": []})),
        ("GetTaskPushNotificationConfig", json!({"taskId": 7})),
        ("ListTaskPushNotificationConfigs", json!({"taskId": {}})),
        ("DeleteTaskPushNotificationConfig", json!({"id": false})),
    ]
    .into_iter()
    .map(|(method, params)| json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}));
    malformed
        .chain(handler_refused_calls().into_iter().map(|(call, _)| call))
        .collect()
}

#[tokio::test]
async fn websocket_malformed_frame_is_refused_without_credential() {
    let (handler, checked) = gated();
    let addr = Arc::new(WebSocketDispatcher::new(handler))
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("serve");
    let mut misses = Misses::default();
    for token in TOKENS {
        let mut req = format!("ws://{addr}").into_client_request().expect("url");
        let headers = req.headers_mut();
        headers.insert("a2a-version", "1.0".parse().expect("value"));
        if let Some(token) = token {
            let value = format!("Bearer {token}").parse().expect("value");
            headers.insert("authorization", value);
        }
        let (mut ws, _) = tokio_tungstenite::connect_async(req)
            .await
            .expect("connect");
        for frame in frames() {
            ws.send(Message::Text(frame.to_string().into()))
                .await
                .expect("send");
            let v: Value = loop {
                if let Message::Text(t) = ws.next().await.expect("open").expect("frame") {
                    break serde_json::from_str(&t).expect("JSON");
                }
            };
            let right = if token == Some("good") {
                v["error"]["code"] == -32602
            } else {
                v["error"]["code"] == -32600
                    && v["error"]["data"].as_array().is_some_and(|d| {
                        d.iter().any(|d| {
                            d["domain"] == "a2a-rust.com"
                                && d["reason"] == "UNAUTHENTICATED"
                                && d["metadata"]["www-authenticate"] == CHALLENGE
                        })
                    })
            };
            misses.check(right, || {
                format!(
                    "{} {} with {token:?}: {v}",
                    frame["method"], frame["params"]
                )
            });
        }
    }
    misses.assert_none();
    assert_eq!(checked.load(Ordering::SeqCst), 3 * frames().len());
}
