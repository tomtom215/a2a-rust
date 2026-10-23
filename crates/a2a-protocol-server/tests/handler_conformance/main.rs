// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Handler-wide conformance: requirements that hold for every operation, or
//! for every binding, each driven through every `on_*` method or every HTTP
//! binding rather than through the one the requirement was first tested on.
//!
//! | module | spec | what |
//! |---|---|---|
//! | `required_extensions` | §3.3.4, §4.6.3 | every operation refuses a client that does not declare a required extension |
//! | `extension_activation` | §4.6.3 | the executor sees only the extensions the card declares |
//! | `required_fields` | §5.7 | a missing or empty REQUIRED field is `InvalidParams` naming it |
//! | `send_refusals` | §3.1.1, §3.1.2 | content type, terminal and unknown task on both sends |
//! | `not_found_wire` | §3.3.2 | another caller's task and an unknown one answer alike, byte for byte |
//! | `auth_before_store` | §13.1 | a refused call reads no store |
//! | `cancel_and_push` | §3.1.5, §3.1.10 | a second cancel; no delivery after a config is deleted |
//! | `stream_and_push` | §3.1.2, §3.1.7, §3.1.8 | progress before completion; a config read back whole and kept |

#[path = "../support/s3_agent.rs"]
mod s3_agent;

mod auth_before_store;
mod cancel_and_push;
mod extension_activation;
mod not_found_wire;
mod required_extensions;
mod required_fields;
mod send_refusals;
mod stream_and_push;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use a2a_protocol_types::agent_card::AgentCard;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use serde_json::{Value, json};

/// Generous: only a hang outlasts it.
pub const DEADLINE: Duration = Duration::from_secs(10);

/// A card declaring every optional capability, the given extensions
/// (`(uri, required)`), and `text/plain` as its only input mode.
pub fn card(extensions: &[(&str, bool)]) -> AgentCard {
    let extensions: Vec<Value> = extensions
        .iter()
        .map(|(uri, required)| json!({"uri": uri, "required": required}))
        .collect();
    serde_json::from_value(json!({
        "name": "conformance", "description": "d", "version": "1",
        "supportedInterfaces": [
            {"url": "http://127.0.0.1:1", "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}
        ],
        "capabilities": {
            "streaming": true, "pushNotifications": true, "extendedAgentCard": true,
            "extensions": extensions
        },
        "defaultInputModes": ["text/plain"], "defaultOutputModes": ["text/plain"],
        "skills": [{"id": "s", "name": "S", "description": "d", "tags": ["t"]}]
    }))
    .expect("card")
}

/// A header map as the dispatchers build it: lower-case keys.
pub fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), (*v).to_owned()))
        .collect()
}

/// One HTTP exchange as the wire carried it.
#[derive(Debug)]
pub struct Answer {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Answer {
    /// The body as JSON; panics, naming the body, when it is not.
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("not a JSON body ({e}): {}", self.body))
    }
}

/// Sends one request and reads the whole answer, or as much of a stream as
/// arrives within a short window (a streaming answer does not end by
/// itself while its task runs).
pub async fn exchange(
    addr: SocketAddr,
    verb: &str,
    path: &str,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> Answer {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let mut req = hyper::Request::builder()
        .method(verb)
        .uri(format!("http://{addr}{path}"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0");
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let body = body.map_or_else(Bytes::new, |b| Bytes::from(b.to_string()));
    let resp = tokio::time::timeout(
        DEADLINE,
        client.request(req.body(Full::new(body)).expect("request")),
    )
    .await
    .expect("an answer in time")
    .expect("response");
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                v.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let mut body = resp.into_body();
    let mut bytes = Vec::new();
    // A unary answer ends well inside the window; a stream is cut at it.
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        while let Some(frame) = body.frame().await {
            if let Ok(data) = frame.expect("a frame").into_data() {
                bytes.extend_from_slice(&data);
            }
        }
    })
    .await;
    Answer {
        status,
        headers,
        body: String::from_utf8(bytes).expect("UTF-8 body"),
    }
}

/// A JSON-RPC call of `method` with `params`.
pub async fn jsonrpc(
    addr: SocketAddr,
    method: &str,
    params: &Value,
    extra: &[(&str, &str)],
) -> Answer {
    let envelope = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    exchange(addr, "POST", "/", Some(&envelope), extra).await
}

/// The `data` of the first SSE event of a body, as JSON.
pub fn first_sse_data(body: &str) -> Value {
    let line = body
        .lines()
        .find_map(|l| l.strip_prefix("data:"))
        .unwrap_or_else(|| panic!("no SSE data line in {body:?}"));
    serde_json::from_str(line.trim()).expect("SSE data is JSON")
}
