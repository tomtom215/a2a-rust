// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The JSON-RPC and HTTP+JSON bindings of the scripted peer: HTTP/1.1 over a
//! raw socket, so the peer controls every byte — including the ones a proper
//! server would never write, and the moment it stops writing them.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::{Script, hold_open, working_event};

#[derive(Debug, Clone, Copy)]
pub(super) enum Flavour {
    JsonRpc,
    Rest,
}

const UNAUTHORIZED: &str = "HTTP/1.1 401 Unauthorized\r\nwww-authenticate: Bearer\r\n\
                            content-length: 0\r\nconnection: close\r\n\r\n";
const SSE_HEAD: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                        transfer-encoding: chunked\r\nconnection: close\r\n\r\n";

/// The parts of a request the scripts need: its target and its body.
struct Request {
    target: String,
    body: String,
}

async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut acc = Vec::new();
    let mut buf = [0_u8; 8192];
    loop {
        let n = stream.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        acc.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&acc);
        let Some(end) = text.find("\r\n\r\n") else {
            continue;
        };
        let head = &text[..end];
        let length = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        if acc.len() >= end + 4 + length {
            let target = head.lines().next()?.split(' ').nth(1)?.to_owned();
            let body = String::from_utf8_lossy(&acc[end + 4..end + 4 + length]).into_owned();
            return Some(Request { target, body });
        }
    }
}

fn chunk(s: &str) -> String {
    format!("{:x}\r\n{s}\r\n", s.len())
}

/// A unary call: no answer, a dropped connection, or a body that is not JSON.
async fn answer_unary(script: Script, stream: &mut TcpStream) {
    match script {
        Script::MisFrame { .. } => {
            let body = "{not json";
            let _ = stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
            hold_open().await;
        }
        Script::CutOff { .. } => {}
        _ => hold_open().await,
    }
}

pub(super) async fn serve(flavour: Flavour, script: Script, mut stream: TcpStream) {
    let Some(req) = read_request(&mut stream).await else {
        return;
    };
    if script == Script::Unauthorized {
        let _ = stream.write_all(UNAUTHORIZED.as_bytes()).await;
        return;
    }
    let rpc: serde_json::Value = serde_json::from_str(&req.body).unwrap_or_default();
    let id = rpc.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let streaming = match flavour {
        Flavour::JsonRpc => matches!(
            rpc.get("method").and_then(serde_json::Value::as_str),
            Some("SendStreamingMessage" | "SubscribeToTask")
        ),
        Flavour::Rest => req.target.contains(":stream") || req.target.contains(":subscribe"),
    };
    let frame = |event: serde_json::Value| -> String {
        let payload = match flavour {
            Flavour::JsonRpc => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": event }),
            Flavour::Rest => event,
        };
        chunk(&format!("data: {payload}\n\n"))
    };

    if !streaming {
        answer_unary(script, &mut stream).await;
        return;
    }

    let after = match script {
        Script::Stall { after } | Script::CutOff { after } | Script::MisFrame { after } => after,
        Script::Unauthorized => 0,
    };
    if stream.write_all(SSE_HEAD.as_bytes()).await.is_err() {
        return;
    }
    for _ in 0..after {
        if stream
            .write_all(frame(working_event()).as_bytes())
            .await
            .is_err()
        {
            return;
        }
    }
    match script {
        Script::CutOff { .. } => {
            // Dropped mid-body: no terminating chunk, no final event.
            let _ = stream.shutdown().await;
        }
        Script::MisFrame { .. } => {
            let _ = stream
                .write_all(chunk("data: {\"statusUpdate\": 42\n\n").as_bytes())
                .await;
            hold_open().await;
        }
        _ => hold_open().await,
    }
}
