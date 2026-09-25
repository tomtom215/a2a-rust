// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `tck-client-parse` behaviour: ACTS §10 client tests.
//!
//! A client test hands the agent a canonical wire payload and asks what this
//! SDK's *client* makes of it. The runner sends a `send_message` whose text
//! part names `tck-client-parse` and whose data part is
//! `{operation, wire_payload}`; the agent answers with a completed task
//! carrying, in a data part, whatever the client parsed.
//!
//! The payload is served by a loopback fixture, and a real
//! `a2a_protocol_client` client calls it — so the HTTP exchange, the
//! JSON-RPC envelope and id checks, and the decoding into this SDK's types
//! all run, as they would against a real agent. A fixture that answered with
//! a value already decoded would test nothing but `serde`.
//!
//! The fixture answers every request with the payload's `result` (or, for a
//! payload that is not an envelope, the payload itself) in a fresh JSON-RPC
//! envelope carrying the request's own id: the corpus's ids (`"req-001"`)
//! cannot match the id the client chose, and a client that checks ids is
//! right to.

use std::net::SocketAddr;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::message::{Message, MessageId, Part, PartContent};
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};

/// Reads the `{operation, wire_payload}` request from the message's data
/// part.
pub(crate) fn request(message: &Message) -> Result<(String, Value), String> {
    let data = message
        .parts
        .iter()
        .find_map(|p| match &p.content {
            PartContent::Data(v) => Some(v),
            _ => None,
        })
        .ok_or("tck-client-parse needs a data part with {operation, wire_payload}")?;
    let operation = data
        .get("operation")
        .and_then(Value::as_str)
        .ok_or("tck-client-parse: `operation` is missing")?
        .to_owned();
    let payload = data
        .get("wire_payload")
        .cloned()
        .ok_or("tck-client-parse: `wire_payload` is missing")?;
    Ok((operation, payload))
}

/// Runs `operation` through this SDK's client against a fixture that serves
/// `payload`, and returns what the client parsed, as JSON.
pub(crate) async fn parse(operation: &str, payload: Value) -> Result<Value, String> {
    let (addr, fixture) = serve(payload).await?;
    let base = format!("http://{addr}");
    let result = drive(operation, &base).await;
    fixture.abort();
    result
}

fn reserialize(e: serde_json::Error) -> String {
    format!("re-serializing: {e}")
}

async fn drive(operation: &str, base: &str) -> Result<Value, String> {
    match operation {
        "get_agent_card" => {
            let card = a2a_protocol_client::discovery::resolve_agent_card(base)
                .await
                .map_err(|e| format!("client refused the card: {e}"))?;
            serde_json::to_value(&card).map_err(reserialize)
        }
        other => {
            let client = ClientBuilder::new(base)
                .build()
                .map_err(|e| format!("client: {e}"))?;
            match other {
                "send_message" => {
                    let message = Message::user(
                        MessageId::new("client-parse"),
                        vec![Part::text("client parse")],
                    );
                    let resp = client
                        .send_message(MessageSendParams {
                            tenant: None,
                            message,
                            configuration: None,
                            metadata: None,
                        })
                        .await
                        .map_err(|e| format!("client refused the response: {e}"))?;
                    serde_json::to_value(&resp).map_err(reserialize)
                }
                "get_task" => {
                    let task = client
                        .get_task(TaskQueryParams::new("client-parse"))
                        .await
                        .map_err(|e| format!("client refused the response: {e}"))?;
                    serde_json::to_value(&task).map_err(reserialize)
                }
                "get_extended_agent_card" => {
                    let card = client
                        .get_extended_agent_card()
                        .await
                        .map_err(|e| format!("client refused the response: {e}"))?;
                    serde_json::to_value(&card).map_err(reserialize)
                }
                _ => Err(format!("tck-client-parse: no client call for {other}")),
            }
        }
    }
}

/// The body the fixture sends for a request whose JSON-RPC id is `id`.
fn answer(payload: &Value, id: &Value, is_card_path: bool) -> Value {
    let inner = payload
        .get("result")
        .filter(|_| payload.get("jsonrpc").is_some())
        .unwrap_or(payload);
    if is_card_path {
        return inner.clone();
    }
    json!({"jsonrpc": "2.0", "id": id, "result": inner})
}

/// A loopback HTTP/1.1 listener answering every request from `payload`.
/// Aborting the returned handle stops it.
async fn serve(payload: Value) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("fixture bind: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("fixture addr: {e}"))?;
    let handle = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let payload = payload.clone();
            tokio::spawn(async move {
                let Some((head, body)) = read_request(&mut stream).await else {
                    return;
                };
                let is_card_path = head
                    .lines()
                    .next()
                    .is_some_and(|l| l.contains("/.well-known/agent-card.json"));
                let id = serde_json::from_slice::<Value>(&body)
                    .ok()
                    .and_then(|v| v.get("id").cloned())
                    .unwrap_or(Value::Null);
                let body = answer(&payload, &id, is_card_path).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    Ok((addr, handle))
}

/// Reads one request: the head as text, and a body of `Content-Length`
/// bytes. `None` on anything malformed or over 1 MiB.
async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<(String, Vec<u8>)> {
    const LIMIT: usize = 1 << 20;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > LIMIT {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0)
        .min(LIMIT);
    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some((head, body))
}

#[cfg(test)]
mod tests {
    use super::{answer, parse};
    use serde_json::json;

    #[test]
    fn an_envelope_is_re_issued_under_the_request_id() {
        let payload = json!({"jsonrpc": "2.0", "id": "req-001", "result": {"x": 1}});
        let body = answer(&payload, &json!(7), false);
        assert_eq!(body, json!({"jsonrpc": "2.0", "id": 7, "result": {"x": 1}}));
    }

    #[test]
    fn a_card_is_served_bare_on_the_well_known_path() {
        let card = json!({"name": "A"});
        assert_eq!(answer(&card, &json!(null), true), card);
    }

    #[tokio::test]
    async fn the_client_parses_a_golden_task_through_http() {
        let payload = json!({
            "jsonrpc": "2.0", "id": "req-003",
            "result": {
                "id": "task-1", "contextId": "ctx-1",
                "status": {"state": "TASK_STATE_COMPLETED", "timestamp": "2025-05-25T20:00:00Z"}
            }
        });
        let parsed = parse("get_task", payload).await.expect("parsed");
        assert_eq!(parsed["id"], "task-1");
        assert_eq!(parsed["status"]["state"], "TASK_STATE_COMPLETED");
    }
}
