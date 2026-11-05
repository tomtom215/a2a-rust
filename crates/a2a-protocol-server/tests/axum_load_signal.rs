// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Audit H7, ADR 0046, on the axum router: `GET /load`, the
//! `endpoint-load-metrics` header on every response, neither without load
//! reporting, and `"halted":true` on `/ready`.

#![cfg(feature = "axum")]

use std::sync::Arc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::axum_adapter::A2aRouter;
use a2a_protocol_server::{HaltScope, RequestHandler, agent_executor};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

struct Exec;
agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

async fn serve(reporting: bool) -> (String, Arc<RequestHandler>) {
    let builder = RequestHandlerBuilder::new(Exec)
        .without_agent_card()
        .with_max_concurrent_streams(8);
    let builder = if reporting {
        builder.with_load_reporting()
    } else {
        builder
    };
    let handler = Arc::new(builder.build().expect("handler"));
    let app = A2aRouter::new(Arc::clone(&handler)).into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), handler)
}

/// Status, the load header if any, and the body as JSON (or `null`).
async fn get(url: &str) -> (u16, Option<String>, serde_json::Value) {
    let client = Client::builder(TokioExecutor::new()).build_http::<Empty<Bytes>>();
    let req = hyper::Request::get(url).body(Empty::new()).unwrap();
    let resp = client.request(req).await.unwrap();
    let status = resp.status().as_u16();
    let header = resp
        .headers()
        .get("endpoint-load-metrics")
        .map(|v| v.to_str().unwrap().to_owned());
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, header, json)
}

const IDLE: &str =
    "TEXT application_utilization=0.000, named_metrics.running=0, named_metrics.queued=0";

#[tokio::test]
async fn the_router_serves_load_and_stamps_every_response() {
    let (base, _) = serve(true).await;
    let (status, header, body) = get(&format!("{base}/load")).await;
    assert_eq!(status, 200);
    assert_eq!(header.as_deref(), Some(IDLE));
    assert_eq!(
        body,
        serde_json::json!({
            "running": 0, "queued": 0, "capacity": 8, "utilization": 0.0,
            "draining": false, "halted": false,
        })
    );
    let (_, header, _) = get(&format!("{base}/health")).await;
    assert_eq!(header.as_deref(), Some(IDLE));
}

#[tokio::test]
async fn without_load_reporting_the_router_has_neither() {
    let (base, _) = serve(false).await;
    let (status, header, _) = get(&format!("{base}/load")).await;
    assert_eq!(status, 404);
    assert_eq!(header, None);
    let (_, header, _) = get(&format!("{base}/health")).await;
    assert_eq!(header, None);
}

#[tokio::test]
async fn the_routers_ready_says_halted() {
    let (base, handler) = serve(false).await;
    let (status, _, body) = get(&format!("{base}/ready")).await;
    assert_eq!(
        (status, body),
        (200, serde_json::json!({"status": "ready"}))
    );
    handler.halt(HaltScope::All, "op", "everything").await;
    let (status, _, body) = get(&format!("{base}/ready")).await;
    assert_eq!(
        (status, body),
        (200, serde_json::json!({"status": "ready", "halted": true}))
    );
}
