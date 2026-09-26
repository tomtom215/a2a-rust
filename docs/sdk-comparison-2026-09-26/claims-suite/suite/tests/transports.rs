// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: Quad transport (JSON-RPC, REST, WebSocket, gRPC client+server),
//! SSE streaming with broadcast multi-subscriber, SSE Last-Event-ID resumption.
//! Ports 7500-7549.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::client::{WebSocketTransport};
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{GrpcConfig, GrpcDispatcher, WebSocketDispatcher};
use claims_suite::common::*;

fn port() -> u16 {
    port_in(7500, 50)
}

async fn exercise(client: &A2aClient, label: &str) {
    // SendMessage
    let t = task_of(client.send_message(msg(&uid(), "Tom")).await.unwrap_or_else(|e| panic!("{label} send: {e}")));
    assert_eq!(t.text(), Some("Hello, Tom!"), "{label}: task text");
    assert_eq!(t.status.state, TaskState::Completed, "{label}");
    // GetTask
    let got = client
        .get_task(TaskQueryParams::new(t.id.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{label} get: {e}"));
    assert_eq!(got.id, t.id, "{label} get id");
    // Streaming
    let mut s = client.stream_message(msg(&uid(), "Ana")).await.unwrap_or_else(|e| panic!("{label} stream: {e}"));
    let mut states = vec![];
    let mut arts = vec![];
    while let Some(ev) = s.next().await {
        match ev.unwrap_or_else(|e| panic!("{label} stream ev: {e}")) {
            StreamResponse::StatusUpdate(u) => states.push(u.status.state),
            StreamResponse::ArtifactUpdate(a) => arts.push(a.artifact.id.to_string()),
            _ => {}
        }
    }
    println!("{label}: states={states:?} artifacts={arts:?}");
    assert!(states.contains(&TaskState::Working), "{label} working");
    assert_eq!(states.last(), Some(&TaskState::Completed), "{label} completed last");
    assert_eq!(arts, vec!["greeting".to_string()], "{label} artifact");
    // CancelTask on a running task
    let mut s = client.stream_message(msg(&uid(), "sleep:5000")).await.unwrap();
    let tid = loop {
        match s.next().await.unwrap().unwrap() {
            StreamResponse::Task(t) => break t.id.to_string(),
            StreamResponse::StatusUpdate(u) => break u.task_id.to_string(),
            _ => {}
        }
    };
    let c = client.cancel_task(tid.clone()).await.unwrap_or_else(|e| panic!("{label} cancel: {e}"));
    println!("{label}: cancel -> {:?}", c.status.state);
    assert_eq!(c.status.state, TaskState::Canceled, "{label} canceled");
    println!("{label}: OK");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quad_transport_client_and_server() {
    let (agent, _probe) = CtlAgent::new();
    let handler = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());

    let (p1, p2, p3, p4) = (port(), port(), port(), port());
    serve_with_addr(format!("127.0.0.1:{p1}"), JsonRpcDispatcher::new(handler.clone())).await.unwrap();
    serve_with_addr(format!("127.0.0.1:{p2}"), RestDispatcher::new(handler.clone())).await.unwrap();
    Arc::new(WebSocketDispatcher::new(handler.clone()))
        .serve_with_addr(format!("127.0.0.1:{p3}"))
        .await
        .unwrap();
    GrpcDispatcher::new(handler.clone(), GrpcConfig::default())
        .serve_with_addr(format!("127.0.0.1:{p4}"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let jsonrpc = ClientBuilder::new(format!("http://127.0.0.1:{p1}")).build().unwrap();
    exercise(&jsonrpc, "JSONRPC").await;

    let rest = ClientBuilder::new(format!("http://127.0.0.1:{p2}"))
        .with_protocol_binding("REST")
        .build()
        .unwrap();
    exercise(&rest, "REST").await;

    let ws_t = WebSocketTransport::connect(format!("ws://127.0.0.1:{p3}")).await.expect("ws connect");
    let ws = ClientBuilder::new(format!("ws://127.0.0.1:{p3}"))
        .with_custom_transport(ws_t)
        .build()
        .unwrap();
    exercise(&ws, "WEBSOCKET").await;

    let grpc = ClientBuilder::new(format!("http://127.0.0.1:{p4}")).build_grpc().await.expect("grpc build");
    exercise(&grpc, "GRPC").await;
}

/// Broadcast multi-subscriber: two SubscribeToTask streams + the originating
/// stream all see the same live events.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sse_multi_subscriber() {
    let (agent, _probe) = CtlAgent::new();
    let handler = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());
    let p = port();
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(handler)).await.unwrap();
    let client = Arc::new(ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap());

    let mut s0 = client.stream_message(msg(&uid(), "chunks:6:300")).await.unwrap();
    let tid = loop {
        match s0.next().await.unwrap().unwrap() {
            StreamResponse::Task(t) => break t.id.to_string(),
            StreamResponse::StatusUpdate(u) => break u.task_id.to_string(),
            _ => {}
        }
    };
    let mut handles = vec![];
    for i in 0..2 {
        let c = client.clone();
        let tid = tid.clone();
        handles.push(tokio::spawn(async move {
            let mut s = c.subscribe_to_task(tid).await.expect("subscribe");
            let mut seen = vec![];
            while let Some(ev) = s.next().await {
                match ev.expect("sub ev") {
                    StreamResponse::ArtifactUpdate(a) => seen.push(a.artifact.id.to_string()),
                    StreamResponse::StatusUpdate(u) => seen.push(format!("{:?}", u.status.state)),
                    StreamResponse::Task(_) => seen.push("TaskSnapshot".into()),
                    _ => {}
                }
            }
            println!("subscriber {i}: {seen:?}");
            seen
        }));
    }
    let mut seen0 = vec![];
    while let Some(ev) = s0.next().await {
        if let StreamResponse::ArtifactUpdate(a) = ev.unwrap() {
            seen0.push(a.artifact.id.to_string());
        }
    }
    println!("originator artifacts: {seen0:?}");
    assert_eq!(seen0.len(), 6);
    for h in handles {
        let seen = h.await.unwrap();
        assert_eq!(seen.first().map(String::as_str), Some("TaskSnapshot"));
        assert!(seen.contains(&"chunk-5".to_string()), "subscriber saw final chunk");
        assert_eq!(seen.last().map(String::as_str), Some("Completed"));
    }
}

/// SSE Last-Event-ID resumption over JSON-RPC and REST (raw HTTP), and via
/// client.subscribe_to_task_from.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sse_last_event_id_resumption() {
    let (agent, _probe) = CtlAgent::new();
    let handler = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());
    let p = port();
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(handler)).await.unwrap();
    let client = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();

    // 8 chunks, 400ms apart; read the first few, then disconnect.
    let mut s = client.stream_message(msg(&uid(), "chunks:8:400")).await.unwrap();
    let mut tid = String::new();
    let mut first = vec![];
    while first.len() < 3 {
        match s.next().await.unwrap().unwrap() {
            StreamResponse::Task(t) => tid = t.id.to_string(),
            StreamResponse::ArtifactUpdate(a) => first.push(a.artifact.id.to_string()),
            _ => {}
        }
    }
    let last = s.last_event_id().map(str::to_owned);
    println!("task={tid} first={first:?} last_event_id={last:?}");
    let last = last.expect("SSE stream must expose last_event_id");
    drop(s);
    // Let two more chunks be emitted while disconnected.
    tokio::time::sleep(Duration::from_millis(900)).await;

    let mut s2 = client.subscribe_to_task_from(tid.clone(), last.clone()).await.expect("resubscribe");
    let mut rest = vec![];
    while let Some(ev) = s2.next().await {
        match ev.unwrap() {
            StreamResponse::ArtifactUpdate(a) => rest.push(a.artifact.id.to_string()),
            StreamResponse::StatusUpdate(u) => rest.push(format!("{:?}", u.status.state)),
            _ => {}
        }
    }
    println!("resumed: {rest:?}");
    let arts: Vec<_> = rest.iter().filter(|x| x.starts_with("chunk-")).cloned().collect();
    let mut all = first.clone();
    all.extend(arts.clone());
    let expect: Vec<String> = (0..8).map(|i| format!("chunk-{i}")).collect();
    assert_eq!(all, expect, "no gaps, no duplicates across the resume");
    assert_eq!(rest.last().map(String::as_str), Some("Completed"));
}
