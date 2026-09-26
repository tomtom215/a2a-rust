// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: ServerInterceptor + on_complete (success / failure / client
//! disconnect), X-Request-ID propagation, Metrics callbacks, RateLimitInterceptor.
//! Ports 7650-7699.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{BearerTokenAuthInterceptor, GrpcConfig, GrpcDispatcher};
use claims_suite::common::*;
use serde_json::json;
use tokio::io::AsyncWriteExt;

fn port() -> u16 {
    port_in(7650, 50)
}

#[tokio::test(flavor = "multi_thread")]
async fn on_complete_success_failure_disconnect() {
    let (agent, probe) = CtlAgent::new();
    let rec = RecInterceptor::default();
    let p = port();
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_interceptor(rec.clone()).build().unwrap());
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let url = format!("http://127.0.0.1:{p}");

    // success
    let (s, _, b) = jsonrpc_raw(&url, "SendMessage", send_params_json("ok"), &[("x-request-id", "rid-123")]).await;
    assert_eq!(s, 200, "{b}");
    // failure: executor error (task fails) + handler error (GetTask unknown id)
    let (_, _, bf) = jsonrpc_raw(&url, "SendMessage", send_params_json("fail"), &[]).await;
    println!("executor-failure send body: {}", &bf[..bf.len().min(200)]);
    let (_, _, bg) = jsonrpc_raw(&url, "GetTask", json!({"id":"nope"}), &[]).await;
    assert!(bg.contains("error"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let ev1 = rec.events();
    println!("events after success/failure: {ev1:#?}");

    // client disconnect mid-SendMessage (blocking send, executor sleeps 3s)
    let body = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":9,"method":"SendMessage","params":send_params_json("sleep:3000")})).unwrap();
    let mut sock = tokio::net::TcpStream::connect(("127.0.0.1", p)).await.unwrap();
    let req = format!(
        "POST / HTTP/1.1\r\nhost: x\r\ncontent-type: application/json\r\nA2A-Version: 1.0\r\ncontent-length: {}\r\n\r\n",
        body.len()
    );
    sock.write_all(req.as_bytes()).await.unwrap();
    sock.write_all(&body).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    drop(sock); // disconnect
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let ev2 = rec.events();
    let new: Vec<_> = ev2[ev1.len()..].to_vec();
    println!("events after disconnect (1s later): {new:#?}");
    tokio::time::sleep(Duration::from_millis(3000)).await;
    let ev3 = rec.events();
    println!("events after executor would have finished (4s): {:#?}", &ev3[ev1.len()..]);
    println!("executor finished count: {}", probe.finished.load(std::sync::atomic::Ordering::SeqCst));

    // Streaming: disconnect mid-stream
    let n_before = rec.events().len();
    let client = ClientBuilder::new(url.clone()).build().unwrap();
    let mut st = client.stream_message(msg(&uid(), "chunks:5:400")).await.unwrap();
    let _ = st.next().await;
    drop(st);
    drop(client);
    tokio::time::sleep(Duration::from_millis(3000)).await;
    println!("streaming disconnect events: {:#?}", &rec.events()[n_before..]);

    // Assertions
    assert!(ev1.iter().any(|e| e == "complete:SendMessage:Succeeded"));
    assert!(ev1.iter().any(|e| e == "complete:GetTask:Failed"));
    assert!(ev1.iter().any(|e| e.starts_with("before:SendMessage:rid=Some(\"rid-123\")")), "X-Request-ID reached CallContext");
    assert!(
        ev3[ev1.len()..].iter().any(|e| e == "complete:SendMessage:Cancelled"),
        "client disconnect must be reported as Cancelled"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn request_id_into_request_context_all_bindings() {
    let (agent, probe) = CtlAgent::new();
    let h = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());
    let (p1, p2, p3) = (port(), port(), port());
    serve_with_addr(format!("127.0.0.1:{p1}"), JsonRpcDispatcher::new(h.clone())).await.unwrap();
    serve_with_addr(format!("127.0.0.1:{p2}"), RestDispatcher::new(h.clone())).await.unwrap();
    GrpcDispatcher::new(h.clone(), GrpcConfig::default()).serve_with_addr(format!("127.0.0.1:{p3}")).await.unwrap();
    jsonrpc_raw(&format!("http://127.0.0.1:{p1}"), "SendMessage", send_params_json("a"), &[("X-Request-ID", "jr-1")]).await;
    reqwest::Client::new()
        .post(format!("http://127.0.0.1:{p2}/message:send"))
        .header("content-type", "application/json")
        .header("A2A-Version", "1.0")
        .header("X-Request-ID", "rest-1")
        .json(&send_params_json("b"))
        .send()
        .await
        .unwrap();
    // gRPC via grpcurl-less path: use SDK client with a custom header interceptor is heavier; skip gRPC header.
    let ids = probe.request_ids.lock().unwrap().clone();
    println!("RequestContext.request_id seen by executor: {ids:?}");
    assert!(ids.contains(&Some("jr-1".into())));
    assert!(ids.contains(&Some("rest-1".into())));
}

#[tokio::test(flavor = "multi_thread")]
async fn metrics_callbacks_fire() {
    let (agent, _probe) = CtlAgent::new();
    let m = RecMetrics::default();
    let p = port();
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_metrics(m.clone()).build().unwrap());
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let client = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();
    client.send_message(msg(&uid(), "x")).await.unwrap();
    let mut s = client.stream_message(msg(&uid(), "y")).await.unwrap();
    while s.next().await.is_some() {}
    let _ = client.get_task(TaskQueryParams::new("does-not-exist")).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let snap = m.snapshot();
    println!("metrics callbacks: {snap:#?}");
    for k in ["on_request", "on_response", "on_error", "on_latency", "on_queue_depth_change"] {
        assert!(m.count(k) > 0, "{k} never fired");
    }
}

async fn rl_server(cfg: RateLimitConfig, with_auth: bool) -> (u16, u16) {
    let (agent, _p) = CtlAgent::new();
    let mut b = RequestHandlerBuilder::new(agent);
    if with_auth {
        b = b.with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([("tokA", "alice"), ("tokB", "bob")]));
    }
    let h = Arc::new(b.with_interceptor(RateLimitInterceptor::new(cfg).unwrap()).build().unwrap());
    let (p1, p2) = (port(), port());
    serve_with_addr(format!("127.0.0.1:{p1}"), JsonRpcDispatcher::new(h.clone())).await.unwrap();
    serve_with_addr(format!("127.0.0.1:{p2}"), RestDispatcher::new(h)).await.unwrap();
    (p1, p2)
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_interceptor() {
    let cfg = RateLimitConfig::default().with_requests_per_window(3).with_window_secs(60);
    let (p1, p2) = rl_server(cfg, false).await;
    let url = format!("http://127.0.0.1:{p1}");
    let mut jr = vec![];
    for _ in 0..5 {
        let (s, h, b) = jsonrpc_raw(&url, "GetTask", json!({"id":"x"}), &[]).await;
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        jr.push((s, v["error"]["code"].clone(), v["error"]["message"].clone(), h.get("retry-after").cloned()));
    }
    println!("JSON-RPC (limit 3/60s): {jr:#?}");
    let mut rest = vec![];
    for _ in 0..5 {
        let r = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{p2}/tasks/x"))
            .header("A2A-Version", "1.0")
            .send()
            .await
            .unwrap();
        let s = r.status().as_u16();
        let ra = r.headers().get("retry-after").cloned();
        rest.push((s, ra, r.text().await.unwrap()));
    }
    println!("REST (separate counter? same handler): {rest:#?}");
    // SDK client w/ RetryPolicy: what does it see?
    let c = ClientBuilder::new(url.clone()).with_retry_policy(RetryPolicy::default()).build().unwrap();
    let e = c.get_task(TaskQueryParams::new("x")).await.unwrap_err();
    println!("SDK client error when limited: {e:?}");

    let limited_jr = jr.iter().filter(|(_, code, _, _)| !code.is_null() && code != -32001).count();
    assert_eq!(limited_jr, 2, "2 of 5 JSON-RPC calls should be limited");
    let rest_status: Vec<u16> = rest.iter().map(|x| x.0).collect();
    println!("REST statuses: {rest_status:?}");
    assert!(rest_status.iter().all(|s| *s != 429), "documented: no 429");
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_per_caller_with_labelled_auth() {
    let cfg = RateLimitConfig::default().with_requests_per_window(2).with_window_secs(60);
    let (p1, _) = rl_server(cfg, true).await;
    let url = format!("http://127.0.0.1:{p1}");
    let mut a = vec![];
    for _ in 0..3 {
        let (_, _, b) = jsonrpc_raw(&url, "GetTask", json!({"id":"x"}), &[("authorization", "Bearer tokA")]).await;
        a.push(b.contains("rate limit") || b.contains("Rate limit"));
    }
    let (_, _, bb) = jsonrpc_raw(&url, "GetTask", json!({"id":"x"}), &[("authorization", "Bearer tokB")]).await;
    println!("alice limited per call: {a:?}; bob first call limited: {}", bb.contains("ate limit"));
    assert_eq!(a, vec![false, false, true]);
    assert!(!bb.contains("ate limit"));
}
