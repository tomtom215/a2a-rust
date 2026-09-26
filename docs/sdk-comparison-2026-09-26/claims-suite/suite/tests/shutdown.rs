// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: Graceful shutdown (Server::serve_with_shutdown, ServeReport),
//! Server::bind max_connections, WebSocket/gRPC serve_with_shutdown.
//! Ports 7700-7749.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{GrpcConfig, GrpcDispatcher, ServeConfig, Server, WebSocketDispatcher};
use claims_suite::common::*;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn port() -> u16 {
    port_in(7700, 50)
}

fn state_of(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let s = &v["result"]["task"]["status"]["state"];
    if s.is_null() {
        format!("NO-TASK: {}", &body[..body.len().min(160)])
    } else {
        s.as_str().unwrap().to_owned()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn graceful_shutdown_http() {
    let (agent, probe) = CtlAgent::new();
    let handler = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());
    let p = port();
    let server = Server::bind(format!("127.0.0.1:{p}")).await.unwrap().with_config(
        ServeConfig::new()
            .with_completion_grace(Duration::from_secs(2))
            .with_task_grace(Duration::from_secs(1))
            .with_drain_timeout(Duration::from_secs(2)),
    );
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let h2 = handler.clone();
    let srv = tokio::spawn(async move {
        server
            .serve_with_shutdown(JsonRpcDispatcher::new(h2), async {
                rx.await.ok();
            })
            .await
    });
    let url = format!("http://127.0.0.1:{p}");
    let mut reqs = vec![];
    for t in ["sleep:800", "sleep:20000", "hang:8000"] {
        let u = url.clone();
        reqs.push(tokio::spawn(async move {
            let r = tokio::time::timeout(Duration::from_secs(12), jsonrpc_raw(&u, "SendMessage", send_params_json(t), &[])).await;
            (t, r.map(|(s, _, b)| (s, state_of(&b))))
        }));
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let t0 = Instant::now();
    tx.send(()).unwrap();
    // New connections after the signal must be refused.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let late = tokio::net::TcpStream::connect(("127.0.0.1", p)).await;
    println!("connect after shutdown signal: {}", if late.is_ok() { "ACCEPTED" } else { "refused" });

    let report = srv.await.unwrap();
    println!("serve_with_shutdown returned after {:?}", t0.elapsed());
    println!("ServeReport = {report:#?}");
    let hr = handler.shutdown().await;
    println!("handler.shutdown() -> {hr:?} graceful={}", hr.is_graceful());
    for r in reqs {
        println!("client result: {:?}", r.await.unwrap());
    }
    println!("executor: finished={} cancels_observed={}", probe.finished.load(Ordering::SeqCst), probe.cancels_observed.load(Ordering::SeqCst));

    let tasks = report.tasks.expect("tasks report");
    assert!(late.is_err(), "listener must be closed");
    assert_eq!(tasks.completed, 1, "sleep:800 finished within completion_grace");
    assert_eq!(tasks.cancelled, 2);
    assert_eq!(tasks.still_running, 1, "hang task ignored cancellation");
    assert!(!tasks.finished);
}

/// max_connections=2: a third concurrent connection is not served until one frees.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_bind_max_connections() {
    let (agent, _probe) = CtlAgent::new();
    let handler = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());
    let p = port();
    let server = Server::bind(format!("127.0.0.1:{p}"))
        .await
        .unwrap()
        .with_config(ServeConfig::new().with_max_connections(2));
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let srv = tokio::spawn(async move {
        server.serve_with_shutdown(JsonRpcDispatcher::new(handler), async { rx.await.ok(); }).await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Two idle keep-alive connections that have completed one request each.
    let mut held = vec![];
    for _ in 0..2 {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", p)).await.unwrap();
        s.write_all(b"GET /.well-known/agent-card.json HTTP/1.1\r\nhost: x\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).await.unwrap();
        assert!(n > 0);
        held.push(s);
    }
    let third = async {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", p)).await.unwrap();
        s.write_all(b"GET /.well-known/agent-card.json HTTP/1.1\r\nhost: x\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_owned()
    };
    let r = tokio::time::timeout(Duration::from_millis(1500), third).await;
    println!("third connection while 2 held: {}", if r.is_err() { "NOT SERVED (waiting)".to_owned() } else { format!("served: {:?}", r) });
    assert!(r.is_err());
    drop(held.pop());
    let third = async {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", p)).await.unwrap();
        s.write_all(b"GET /.well-known/agent-card.json HTTP/1.1\r\nhost: x\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_owned()
    };
    let r = tokio::time::timeout(Duration::from_millis(3000), third).await;
    println!("after releasing one: {r:?}");
    assert!(r.is_ok());
    tx.send(()).unwrap();
    let rep = srv.await.unwrap();
    println!("report: {rep:?}");
}

/// WebSocket + gRPC dispatchers' serve_with_shutdown: the in-flight task phases run.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_and_grpc_serve_with_shutdown() {
    for kind in ["ws", "grpc"] {
        let (agent, probe) = CtlAgent::new();
        let handler = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());
        let p = port();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", p)).await.unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let h2 = handler.clone();
        let srv = tokio::spawn(async move {
            let sig = async { rx.await.ok(); };
            if kind == "ws" {
                Arc::new(WebSocketDispatcher::new(h2)).serve_with_shutdown(listener, sig).await
            } else {
                GrpcDispatcher::new(h2, GrpcConfig::default()).serve_with_shutdown(listener, sig).await.unwrap()
            }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        let client = if kind == "ws" {
            let t = a2a_protocol_sdk::client::WebSocketTransport::connect(format!("ws://127.0.0.1:{p}")).await.unwrap();
            ClientBuilder::new(format!("ws://127.0.0.1:{p}")).with_custom_transport(t).build().unwrap()
        } else {
            ClientBuilder::new(format!("http://127.0.0.1:{p}")).build_grpc().await.unwrap()
        };
        let client = Arc::new(client);
        let c2 = client.clone();
        let inflight = tokio::spawn(async move { c2.send_message(msg(&uid(), "sleep:20000")).await });
        tokio::time::sleep(Duration::from_millis(400)).await;
        let t0 = Instant::now();
        tx.send(()).unwrap();
        let rep = tokio::time::timeout(Duration::from_secs(40), srv).await.expect("returned").unwrap();
        println!("{kind}: serve_with_shutdown returned after {:?}: {rep:?}", t0.elapsed());
        let r = tokio::time::timeout(Duration::from_secs(5), inflight).await;
        let desc = match r {
            Ok(Ok(Ok(SendMessageResponse::Task(t)))) => format!("task state {:?}", t.status.state),
            other => format!("{other:?}"),
        };
        println!("{kind}: in-flight client result: {desc}; executor cancels observed={}", probe.cancels_observed.load(Ordering::SeqCst));
        let tasks = rep.tasks.expect("tasks report");
        assert_eq!(tasks.cancelled, 1, "{kind}");
        assert!(tasks.finished, "{kind}");
    }
}
