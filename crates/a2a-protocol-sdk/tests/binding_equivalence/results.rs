// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `BIND-EQUIV-002` (spec §5.1): "all supported protocols MUST return
//! semantically equivalent results for the same requests."
//!
//! Two readings, both tested. The same request sent over each binding makes
//! a task that differs only in what the server mints per call (ids and
//! timestamps). And one task read through each binding is the same task,
//! field for field, after each binding's wire encoding and this SDK's
//! decoding of it.

use a2a_protocol_types::params::{ListPushConfigsParams, ListTasksParams, TaskQueryParams};
use a2a_protocol_types::parse_iso8601_to_unix_millis;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use serde_json::Value;

use crate::functionality::{HOOK, drain, params, send};
use crate::harness::{Binding, serve};

/// What the server mints per call, and so may differ between two calls
/// that asked the same thing.
const MINTED: [&str; 5] = ["id", "contextId", "taskId", "messageId", "timestamp"];

fn without_minted(mut value: Value) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(map) => {
                for key in MINTED {
                    map.remove(key);
                }
                map.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut value);
    value
}

/// Replaces every `timestamp` with the instant it names, in Unix
/// milliseconds. The JSON bindings carry the string the server stored
/// (`…34.160Z`); gRPC carries a `google.protobuf.Timestamp`, which this
/// SDK's client renders with the fraction's trailing zeros trimmed
/// (`…34.16Z`). Both are RFC 3339 for the same instant, which is what a
/// timestamp means; the strings differ about one run in twenty, whenever
/// the millisecond ends in zero.
fn as_instants(mut value: Value) -> Value {
    fn walk(v: &mut Value) {
        match v {
            Value::Object(map) => {
                for (key, field) in map.iter_mut() {
                    if key == "timestamp" {
                        let at = field.as_str().and_then(parse_iso8601_to_unix_millis);
                        *field = Value::from(at.expect("an RFC 3339 timestamp"));
                    } else {
                        walk(field);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(walk),
            _ => {}
        }
    }
    walk(&mut value);
    value
}

fn all_equal(answers: &[(Binding, Value)], what: &str) {
    let (first, expected) = &answers[0];
    for (binding, answer) in &answers[1..] {
        assert_eq!(
            answer, expected,
            "{what}: {binding:?} and {first:?} answer differently"
        );
    }
}

/// The same `SendMessage` over each binding: the same task, ids aside.
#[tokio::test]
async fn bind_equiv_002_the_same_send_gets_the_same_task_on_every_binding() {
    let agent = serve(false, true).await;
    let mut answers = Vec::new();
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        let task = send(&client, "the same words")
            .await
            .unwrap_or_else(|e| panic!("{binding:?}: {e}"));
        answers.push((binding, without_minted(serde_json::to_value(task).unwrap())));
    }
    // Not vacuous: the answer carries the state and the echoed artifact.
    let task = &answers[0].1;
    assert_eq!(task["status"]["state"], "TASK_STATE_COMPLETED", "{task}");
    assert_eq!(task["artifacts"][0]["parts"][0]["text"], "the same words");
    all_equal(&answers, "SendMessage");
}

/// The same `SendStreamingMessage` over each binding: the same events, in
/// the same order, ids aside.
#[tokio::test]
async fn bind_equiv_002_the_same_stream_carries_the_same_events_on_every_binding() {
    let agent = serve(false, true).await;
    let mut answers = Vec::new();
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        let events = drain(client.stream_message(params("streamed")).await.unwrap())
            .await
            .unwrap_or_else(|e| panic!("{binding:?}: {e}"));
        answers.push((
            binding,
            without_minted(serde_json::to_value(events).unwrap()),
        ));
    }
    assert!(
        answers[0].1.as_array().is_some_and(|e| e.len() >= 3),
        "a task, an artifact and a final status at least: {}",
        answers[0].1
    );
    all_equal(&answers, "SendStreamingMessage");
}

/// One task, read through each binding: `GetTask` (with and without a
/// history limit) and `ListTasks` give the same task, field for field.
#[tokio::test]
async fn bind_equiv_002_one_task_reads_the_same_through_every_binding() {
    let agent = serve(false, true).await;
    let made = send(&agent.client(Binding::JsonRpc, None).await, "read me")
        .await
        .expect("send");
    let id = made.id.0.clone();

    let mut got = Vec::new();
    let mut trimmed = Vec::new();
    let mut listed = Vec::new();
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        let task = client.get_task(TaskQueryParams::new(&id)).await.unwrap();
        got.push((binding, as_instants(serde_json::to_value(task).unwrap())));
        let task = client
            .get_task(TaskQueryParams::new(&id).with_history_length(0))
            .await
            .unwrap();
        trimmed.push((binding, as_instants(serde_json::to_value(task).unwrap())));
        let page = client.list_tasks(ListTasksParams::default()).await.unwrap();
        listed.push((
            binding,
            as_instants(serde_json::to_value(page.tasks).unwrap()),
        ));
    }
    assert_eq!(got[0].1["id"], id.as_str());
    assert!(trimmed[0].1.get("history").is_none(), "{}", trimmed[0].1);
    all_equal(&got, "GetTask");
    all_equal(&trimmed, "GetTask with historyLength 0");
    all_equal(&listed, "ListTasks");
}

/// A push config made over one binding reads back the same through every
/// binding.
#[tokio::test]
async fn bind_equiv_002_one_push_config_reads_the_same_through_every_binding() {
    let agent = serve(false, true).await;
    let rpc = agent.client(Binding::JsonRpc, None).await;
    let task = send(&rpc, "configure me").await.expect("send");
    let id = task.id.0.clone();
    let made = rpc
        .set_push_config(TaskPushNotificationConfig::new(id.as_str(), HOOK))
        .await
        .expect("create");
    let config_id = made.id.clone().expect("id");

    let mut got = Vec::new();
    let mut listed = Vec::new();
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        let config = client.get_push_config(&id, &config_id).await.unwrap();
        got.push((binding, serde_json::to_value(config).unwrap()));
        let page = client
            .list_push_configs(ListPushConfigsParams {
                task_id: id.clone(),
                page_size: None,
                page_token: None,
                tenant: None,
            })
            .await
            .unwrap();
        listed.push((binding, serde_json::to_value(page.configs).unwrap()));
    }
    assert_eq!(got[0].1["url"], HOOK);
    all_equal(&got, "GetTaskPushNotificationConfig");
    all_equal(&listed, "ListTaskPushNotificationConfigs");
}
