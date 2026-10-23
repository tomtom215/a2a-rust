// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What an agent sends is held to `a2a.proto`'s REQUIRED fields (spec §5.7)
//! on every path into the client: each binding's unary answers and streamed
//! events, the agent card discovery and `GetExtendedAgentCard`. Until
//! 2026-10-23 none of it was checked, and over gRPC an absent field read as
//! its zero value: a task with no id, a status in `TASK_STATE_UNSPECIFIED`,
//! an artifact with no parts, or a card with no skills was returned as if
//! valid. Each case here has a control that passes on the same peer.

mod cards;
#[cfg(feature = "grpc")]
mod grpc;
mod json;
mod peers;

use a2a_protocol_client::ClientError;

/// A valid task: the control each refusal is measured against.
fn task() -> serde_json::Value {
    serde_json::json!({
        "id": "t-1", "contextId": "c-1",
        "status": {"state": "TASK_STATE_COMPLETED"},
        "artifacts": [{"artifactId": "a-1", "parts": [{"text": "out"}]}]
    })
}

/// A broken copy of [`task`], and the text its refusal must contain.
type Break = (&'static str, fn(&mut serde_json::Value));

/// Each REQUIRED value of a task the JSON parser still reads (`""`, an
/// absent repeated field, an explicit `UNSPECIFIED`): idx 405, 410, 417-419.
const BROKEN_TASKS: [Break; 4] = [
    ("task: id is required", |t| t["id"] = "".into()),
    ("TASK_STATE_UNSPECIFIED is no state", |t| {
        t["status"]["state"] = "TASK_STATE_UNSPECIFIED".into();
    }),
    ("artifactId is required", |t| {
        t["artifacts"][0]["artifactId"] = "".into();
    }),
    ("artifact must contain at least one part", |t| {
        t["artifacts"][0].as_object_mut().unwrap().remove("parts");
    }),
];

fn broken(break_it: fn(&mut serde_json::Value)) -> serde_json::Value {
    let mut t = task();
    break_it(&mut t);
    t
}

/// `result` is the refusal of an agent's object: `InvalidAgentResponse`
/// whose text names `needle` and `call`.
#[track_caller]
fn assert_refused<T: std::fmt::Debug>(result: Result<T, ClientError>, call: &str, needle: &str) {
    match result {
        Err(ClientError::InvalidAgentResponse(msg)) => {
            assert!(
                msg.contains(needle),
                "{call}: expected {needle:?} in {msg:?}"
            );
            assert!(
                msg.contains(call),
                "{call}: the call is not named in {msg:?}"
            );
        }
        other => panic!("{call}: expected InvalidAgentResponse naming {needle:?}, got {other:?}"),
    }
}
