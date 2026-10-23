// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The JSON bindings: JSON-RPC, HTTP+JSON and WebSocket.

use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_types::{Message, MessageRole, MessageSendParams, Part, TaskQueryParams};

use super::peers::{self, Answer, Http, Peer};
use super::{BROKEN_TASKS, assert_refused, broken, task};

#[derive(Clone, Copy, Debug)]
enum Binding {
    JsonRpc,
    Rest,
    #[cfg(feature = "websocket")]
    WebSocket,
}

const BINDINGS: &[Binding] = &[
    Binding::JsonRpc,
    Binding::Rest,
    #[cfg(feature = "websocket")]
    Binding::WebSocket,
];

async fn peer(binding: Binding, answer: Answer) -> Peer {
    match binding {
        Binding::JsonRpc => peers::http(Http::JsonRpc, answer).await,
        Binding::Rest => peers::http(Http::Rest, answer).await,
        #[cfg(feature = "websocket")]
        Binding::WebSocket => peers::ws(answer).await,
    }
}

async fn client(binding: Binding, url: &str) -> A2aClient {
    let builder = ClientBuilder::new(url);
    match binding {
        Binding::JsonRpc => builder.build(),
        Binding::Rest => builder.with_protocol_binding("HTTP+JSON").build(),
        #[cfg(feature = "websocket")]
        Binding::WebSocket => {
            let transport = a2a_protocol_client::transport::WebSocketTransport::connect(url)
                .await
                .expect("ws connect");
            builder.with_custom_transport(transport).build()
        }
    }
    .expect("client")
}

fn query() -> TaskQueryParams {
    TaskQueryParams {
        tenant: None,
        id: "t-1".into(),
        history_length: None,
    }
}

fn send() -> MessageSendParams {
    MessageSendParams::new(Message::new("m", MessageRole::User, vec![Part::text("hi")]))
}

#[tokio::test]
async fn a_task_missing_a_required_value_is_refused_on_every_json_binding() {
    for &binding in BINDINGS {
        let ok = peer(
            binding,
            Answer {
                unary: task(),
                ..Answer::default()
            },
        )
        .await;
        client(binding, &ok.url)
            .await
            .get_task(query())
            .await
            .unwrap_or_else(|e| panic!("{binding:?}: the valid task is refused: {e}"));
        for (needle, break_it) in BROKEN_TASKS {
            let bad = peer(
                binding,
                Answer {
                    unary: broken(break_it),
                    ..Answer::default()
                },
            )
            .await;
            let c = client(binding, &bad.url).await;
            assert_refused(c.get_task(query()).await, "GetTask response", needle);
            assert_refused(c.cancel_task("t-1").await, "CancelTask response", needle);
            let page = serde_json::json!({"tasks": [broken(break_it)], "nextPageToken": "", "pageSize": 1, "totalSize": 1});
            let bad = peer(
                binding,
                Answer {
                    unary: page,
                    ..Answer::default()
                },
            )
            .await;
            let c = client(binding, &bad.url).await;
            assert_refused(
                c.list_tasks(Default::default()).await,
                "ListTasks response",
                needle,
            );
        }
    }
}

#[tokio::test]
async fn a_reply_missing_a_required_value_is_refused() {
    for &binding in BINDINGS {
        let reply = |parts: serde_json::Value| Answer {
            unary: serde_json::json!({"message": {"messageId": "r", "role": "ROLE_AGENT", "parts": parts}}),
            ..Answer::default()
        };
        let ok = peer(binding, reply(serde_json::json!([{"text": "hi"}]))).await;
        client(binding, &ok.url)
            .await
            .send_message(send())
            .await
            .expect("valid reply");
        let bad = peer(binding, reply(serde_json::json!([]))).await;
        assert_refused(
            client(binding, &bad.url).await.send_message(send()).await,
            "SendMessage response",
            "message must contain at least one part",
        );
        let bad = peer(
            binding,
            Answer {
                unary: serde_json::json!({"task": broken(BROKEN_TASKS[1].1)}),
                ..Answer::default()
            },
        )
        .await;
        assert_refused(
            client(binding, &bad.url).await.send_message(send()).await,
            "SendMessage response",
            BROKEN_TASKS[1].0,
        );
    }
}

/// A streamed event is checked as it arrives: the stream ends at it with
/// the refusal, after delivering what came before.
#[tokio::test]
async fn a_streamed_event_missing_a_required_value_ends_the_stream() {
    let working = serde_json::json!({"statusUpdate": {"taskId": "t", "contextId": "c", "status": {"state": "TASK_STATE_WORKING"}}});
    let cases: [(&str, serde_json::Value); 4] = [
        (
            "statusUpdate: contextId is required",
            serde_json::json!({"statusUpdate": {"taskId": "t", "contextId": "", "status": {"state": "TASK_STATE_WORKING"}}}),
        ),
        (
            "TASK_STATE_UNSPECIFIED is no state",
            serde_json::json!({"statusUpdate": {"taskId": "t", "contextId": "c", "status": {"state": "TASK_STATE_UNSPECIFIED"}}}),
        ),
        (
            "artifactUpdate: taskId is required",
            serde_json::json!({"artifactUpdate": {"taskId": "", "contextId": "c", "artifact": {"artifactId": "a", "parts": [{"text": "x"}]}}}),
        ),
        (
            "task: id is required",
            serde_json::json!({"task": broken(BROKEN_TASKS[0].1)}),
        ),
    ];
    for &binding in BINDINGS {
        for (needle, event) in &cases {
            let p = peer(
                binding,
                Answer {
                    events: vec![working.clone(), event.clone()],
                    ..Answer::default()
                },
            )
            .await;
            let mut stream = client(binding, &p.url)
                .await
                .stream_message(send())
                .await
                .expect("stream");
            stream
                .next()
                .await
                .expect("first")
                .expect("the valid event is delivered");
            assert_refused(
                stream.next().await.expect("second"),
                "streamed event",
                needle,
            );
            assert!(
                stream.next().await.is_none(),
                "{binding:?}: the stream ends at the refusal"
            );
        }
    }
}

#[tokio::test]
async fn a_push_config_without_its_url_is_refused() {
    for &binding in BINDINGS {
        let config = |url: &str| Answer {
            unary: serde_json::json!({"id": "p", "taskId": "t-1", "url": url}),
            ..Answer::default()
        };
        let ok = peer(binding, config("https://hook.example/cb")).await;
        client(binding, &ok.url)
            .await
            .get_push_config("t-1", "p")
            .await
            .expect("valid config");
        let bad = peer(binding, config("")).await;
        assert_refused(
            client(binding, &bad.url)
                .await
                .get_push_config("t-1", "p")
                .await,
            "GetTaskPushNotificationConfig response",
            "url is required",
        );
    }
}
