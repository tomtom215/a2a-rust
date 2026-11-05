// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Audit H7, ADR 0046, over a real socket: `GET /load` and the
//! `endpoint-load-metrics` header when load reporting is on, neither when it
//! is off, and `"halted":true` on `/ready` while a halt of every tenant holds.

use std::sync::Arc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{CombinedDispatcher, JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::{Dispatcher, Server};
use a2a_protocol_server::{HaltScope, RequestHandler, agent_executor};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Exec;
agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

fn handler(reporting: bool) -> Arc<RequestHandler> {
    let builder = RequestHandlerBuilder::new(Exec)
        .without_agent_card()
        .with_max_concurrent_streams(8);
    let builder = if reporting {
        builder.with_load_reporting()
    } else {
        builder
    };
    Arc::new(builder.build().expect("handler"))
}

async fn serve(dispatcher: impl Dispatcher) -> u16 {
    let server = Server::bind("127.0.0.1:0").await.expect("bind");
    let port = server.local_addr().expect("addr").port();
    tokio::spawn(server.serve_with_shutdown(dispatcher, std::future::pending()));
    port
}

/// The status, the lower-cased head, and the body of one request.
async fn request(port: u16, method: &str, path: &str, body: &str) -> (u16, String, String) {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.expect("write");
    let mut out = String::new();
    s.read_to_string(&mut out).await.expect("read");
    let (head, rest) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (status, head.to_ascii_lowercase(), rest.to_owned())
}

const IDLE: &str = "endpoint-load-metrics: text application_utilization=0.000, \
                    named_metrics.running=0, named_metrics.queued=0";

#[tokio::test]
async fn load_is_served_and_every_response_carries_it() {
    let port = serve(RestDispatcher::new(handler(true))).await;
    let (status, head, body) = request(port, "GET", "/load", "").await;
    assert_eq!(status, 200, "{body}");
    assert!(head.contains("content-type: application/json"), "{head}");
    let load: serde_json::Value = serde_json::from_str(body.trim()).expect("json");
    assert_eq!(
        load,
        serde_json::json!({
            "running": 0, "queued": 0, "capacity": 8, "utilization": 0.0,
            "draining": false, "halted": false,
        })
    );
    let (_, head, _) = request(port, "GET", "/health", "").await;
    assert!(head.contains(IDLE), "{head}");

    // The JSON-RPC dispatcher's responses carry it too, through the server.
    let port = serve(JsonRpcDispatcher::new(handler(true))).await;
    let (_, head, _) = request(port, "POST", "/", "{}").await;
    assert!(head.contains(IDLE), "{head}");
    // And the combined one routes /load to its HTTP+JSON half.
    let port = serve(CombinedDispatcher::new(handler(true))).await;
    let (status, head, _) = request(port, "GET", "/load", "").await;
    assert_eq!(status, 200);
    assert!(head.contains(IDLE), "{head}");
}

#[tokio::test]
async fn without_load_reporting_there_is_neither_route_nor_header() {
    let port = serve(RestDispatcher::new(handler(false))).await;
    // An unknown path then, answered as any other unknown path is.
    let (status, head, _) = request(port, "GET", "/load", "").await;
    let (unknown, _, _) = request(port, "GET", "/no-such-path", "").await;
    assert_eq!(status, unknown, "{head}");
    assert_ne!(status, 200);
    assert!(!head.contains("endpoint-load-metrics"), "{head}");
    let (_, head, _) = request(port, "GET", "/health", "").await;
    assert!(!head.contains("endpoint-load-metrics"), "{head}");
}

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body.trim()).expect("json")
}

#[tokio::test]
async fn ready_says_halted_and_stays_ready() {
    let h = handler(false);
    let port = serve(RestDispatcher::new(Arc::clone(&h))).await;
    let (status, _, body) = request(port, "GET", "/ready", "").await;
    assert_eq!(status, 200);
    assert_eq!(json(&body), serde_json::json!({"status": "ready"}));
    h.halt(HaltScope::Tenant("acme".into()), "op", "one tenant")
        .await;
    let (_, _, body) = request(port, "GET", "/ready", "").await;
    assert_eq!(
        json(&body),
        serde_json::json!({"status": "ready"}),
        "a tenant's halt is not shown"
    );
    h.halt(HaltScope::All, "op", "everything").await;
    let (status, _, body) = request(port, "GET", "/ready", "").await;
    assert_eq!(
        status, 200,
        "a halted replica still serves reads and cancels"
    );
    assert_eq!(
        json(&body),
        serde_json::json!({"status": "ready", "halted": true})
    );
    h.begin_draining();
    let (status, _, body) = request(port, "GET", "/ready", "").await;
    assert_eq!(status, 503);
    assert_eq!(
        json(&body),
        serde_json::json!({"status": "not_ready", "reason": "draining", "halted": true})
    );
}
