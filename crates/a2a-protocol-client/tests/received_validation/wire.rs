// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the client writes, captured on the wire of each JSON binding.

use a2a_protocol_client::{ClientBuilder, ClientError};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::{
    ErrorCode, ListTasksParams, Message, MessageRole, MessageSendParams, Part,
    SendMessageConfiguration, TaskState,
};

use super::peers::{self, Answer, Http, Peer};

fn page() -> Answer {
    Answer {
        unary: serde_json::json!({"tasks": [], "nextPageToken": "", "pageSize": 0, "totalSize": 0}),
        ..Answer::default()
    }
}

async fn list(peer: &Peer, rest: bool, after: &str) -> Result<(), ClientError> {
    let builder = ClientBuilder::new(&peer.url);
    let builder = if rest {
        builder.with_protocol_binding("HTTP+JSON")
    } else {
        builder
    };
    let params = ListTasksParams {
        status: Some(TaskState::Working),
        status_timestamp_after: Some(after.into()),
        ..ListTasksParams::default()
    };
    builder
        .build()
        .expect("client")
        .list_tasks(params)
        .await
        .map(drop)
}

/// idx 180/184 (spec §5.6.1): `statusTimestampAfter` goes on the wire in
/// UTC with `Z`, whatever offset the caller wrote; and idx 300 (§11.5): in
/// a query, the enum by its name and the timestamp URL-encoded.
#[tokio::test]
async fn list_tasks_sends_status_timestamp_after_in_utc() {
    let rest = peers::http(Http::Rest, page()).await;
    list(&rest, true, "2026-01-02T05:04:05.250+02:00")
        .await
        .expect("list");
    let target = rest.last_request();
    assert!(target.starts_with("GET /tasks?"), "{target}");
    assert!(target.contains("status=TASK_STATE_WORKING"), "{target}");
    assert!(
        target.contains("statusTimestampAfter=2026-01-02T03%3A04%3A05.250Z"),
        "{target}"
    );
    let rpc = peers::http(Http::JsonRpc, page()).await;
    list(&rpc, false, "2026-01-02t03:04:05z")
        .await
        .expect("list");
    let body = rpc.last_request();
    assert!(
        body.contains(r#""statusTimestampAfter":"2026-01-02T03:04:05.000Z""#),
        "{body}"
    );
    #[cfg(feature = "websocket")]
    {
        let ws = peers::ws(page()).await;
        let transport = a2a_protocol_client::transport::WebSocketTransport::connect(&ws.url)
            .await
            .expect("ws connect");
        let params = ListTasksParams {
            status_timestamp_after: Some("2026-01-01T23:04:05-04:00".into()),
            ..ListTasksParams::default()
        };
        ClientBuilder::new(&ws.url)
            .with_custom_transport(transport)
            .build()
            .expect("client")
            .list_tasks(params)
            .await
            .expect("list");
        let frame = ws.last_request();
        assert!(
            frame.contains(r#""statusTimestampAfter":"2026-01-02T03:04:05.000Z""#),
            "{frame}"
        );
    }
    // Already canonical: sent as given, its precision kept.
    let rpc = peers::http(Http::JsonRpc, page()).await;
    list(&rpc, false, "2026-01-02T03:04:05.123456Z")
        .await
        .expect("list");
    assert!(
        rpc.last_request()
            .contains(r#""statusTimestampAfter":"2026-01-02T03:04:05.123456Z""#)
    );
}

#[tokio::test]
async fn list_tasks_refuses_a_timestamp_that_is_not_iso_8601_unsent() {
    let rpc = peers::http(Http::JsonRpc, page()).await;
    match list(&rpc, false, "yesterday").await {
        Err(ClientError::Protocol(e)) => {
            assert_eq!(e.code, ErrorCode::InvalidParams);
            assert!(e.message.contains("statusTimestampAfter"), "{}", e.message);
        }
        other => panic!("expected InvalidParams, got {other:?}"),
    }
    assert!(rpc.seen.lock().unwrap().is_empty(), "nothing is sent");
}

/// idx 401: a2a.proto, `SendMessageConfiguration.task_push_notification_config`:
/// "Task id should be empty when sending this configuration in a
/// SendMessage request".
#[tokio::test]
async fn an_inline_push_configs_task_id_is_not_sent() {
    let reply = Answer {
        unary: serde_json::json!({"message": {"messageId": "r", "role": "ROLE_AGENT", "parts": [{"text": "ok"}]}}),
        ..Answer::default()
    };
    let rpc = peers::http(Http::JsonRpc, reply).await;
    let mut params =
        MessageSendParams::new(Message::new("m", MessageRole::User, vec![Part::text("hi")]));
    params.configuration = Some(SendMessageConfiguration {
        task_push_notification_config: Some(TaskPushNotificationConfig::new(
            "some-other-task",
            "https://hook.example/cb",
        )),
        ..SendMessageConfiguration::default()
    });
    let client = ClientBuilder::new(&rpc.url).build().expect("client");
    client.send_message(params).await.expect("send");
    let body = rpc.last_request();
    assert!(
        body.contains("https://hook.example/cb"),
        "the config is sent: {body}"
    );
    assert!(
        !body.contains("some-other-task"),
        "its taskId is not: {body}"
    );
}
