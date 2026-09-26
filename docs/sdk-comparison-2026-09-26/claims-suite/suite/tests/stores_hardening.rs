// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: SQLite persistence across restart, task eviction TTL/capacity/
//! pagination, /health and /ready, body size limit, Content-Type validation,
//! CORS preflight, executor timeout (default + with_executor_timeout).
//! Ports 7800-7849.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{
    A2aRouter, CorsConfig, DispatchConfig, InMemoryTaskStore, ServeConfig, Server, SqliteTaskStore,
    TaskStore, TaskStoreConfig,
};
use claims_suite::common::*;
use serde_json::{json, Value};

fn port() -> u16 {
    port_in(7800, 50)
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_persists_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("tasks.db").display());
    let p = port();
    let tid;
    {
        let (agent, _) = CtlAgent::new();
        let store = SqliteTaskStore::new(&url).await.unwrap();
        let h = Arc::new(RequestHandlerBuilder::new(agent).with_task_store(store).build().unwrap());
        let server = Server::bind(format!("127.0.0.1:{p}")).await.unwrap().with_config(ServeConfig::new().with_drain_timeout(Duration::from_secs(1)));
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let srv = tokio::spawn(server.serve_with_shutdown(JsonRpcDispatcher::new(h.clone()), async { rx.await.ok(); }));
        let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();
        let t = task_of(c.send_message(msg(&uid(), "persist")).await.unwrap());
        tid = t.id.to_string();
        drop(c);
        tx.send(()).unwrap();
        srv.await.unwrap();
        let _ = h.shutdown().await;
    }
    // "Restart": brand new store + handler + server on the same file.
    let (agent, _) = CtlAgent::new();
    let store = SqliteTaskStore::new(&url).await.unwrap();
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_task_store(store).build().unwrap());
    let p2 = port();
    serve_with_addr(format!("127.0.0.1:{p2}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p2}")).build().unwrap();
    let got = c.get_task(TaskQueryParams::new(tid.clone())).await.expect("task survives restart");
    let list = c.list_tasks(ListTasksParams::default()).await.unwrap();
    println!("after restart: state={:?} text={:?} history_len={:?} listed={}", got.status.state, got.text(), got.history.as_ref().map(Vec::len), list.tasks.len());
    assert_eq!(got.status.state, TaskState::Completed);
    assert_eq!(got.text(), Some("Hello, persist!"));
    assert_eq!(list.tasks.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn task_eviction_ttl_capacity_pagination() {
    let (agent, _) = CtlAgent::new();
    let cfg = TaskStoreConfig::default()
        .with_task_ttl(Some(Duration::from_secs(1)))
        .with_eviction_interval(1);
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_task_store_config(cfg).build().unwrap());
    let p = port();
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();
    let t = task_of(c.send_message(msg(&uid(), "old")).await.unwrap());
    tokio::time::sleep(Duration::from_millis(2200)).await;
    let read_only = c.get_task(TaskQueryParams::new(t.id.to_string())).await;
    let list_only = c.list_tasks(ListTasksParams::default()).await.unwrap();
    println!("TTL 1s, 2.2s later, no writes: get_found={} list_len={}", read_only.is_ok(), list_only.tasks.len());
    let _ = c.send_message(msg(&uid(), "new")).await.unwrap(); // a write triggers the sweep
    let after_write = c.get_task(TaskQueryParams::new(t.id.to_string())).await;
    println!("after a write: old task found={}", after_write.is_ok());
    assert!(after_write.is_err(), "expired terminal task evicted");

    // Capacity + cursor pagination
    let (agent, _) = CtlAgent::new();
    let cfg = TaskStoreConfig::default().with_max_capacity(Some(3)).with_eviction_interval(1);
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_task_store_config(cfg).build().unwrap());
    let p = port();
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();
    for i in 0..6 {
        c.send_message(msg(&uid(), &format!("t{i}"))).await.unwrap();
    }
    let all = c.list_tasks(ListTasksParams::default()).await.unwrap();
    println!("capacity 3 after 6 sends: list_len={}", all.tasks.len());
    assert!(all.tasks.len() <= 3);
    let mut lp = ListTasksParams::default();
    lp.page_size = Some(2);
    let p1 = c.list_tasks(lp.clone()).await.unwrap();
    let mut lp2 = lp.clone();
    lp2.page_token = Some(p1.next_page_token.clone());
    let p2 = c.list_tasks(lp2).await.unwrap();
    println!("pagination: page1={} token={:?} page2={}", p1.tasks.len(), p1.next_page_token, p2.tasks.len());
    let ids1: Vec<_> = p1.tasks.iter().map(|t| t.id.to_string()).collect();
    assert!(p2.tasks.iter().all(|t| !ids1.contains(&t.id.to_string())));
    assert_eq!(p1.tasks.len() + p2.tasks.len(), all.tasks.len());
}

// A store whose readiness can be switched off.
struct FlakyStore {
    inner: InMemoryTaskStore,
    down: Arc<AtomicBool>,
}
type F<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;
impl TaskStore for FlakyStore {
    fn save<'a>(&'a self, t: &'a Task) -> F<'a, ()> { self.inner.save(t) }
    fn get<'a>(&'a self, id: &'a TaskId) -> F<'a, Option<Task>> { self.inner.get(id) }
    fn list<'a>(&'a self, p: &'a ListTasksParams) -> F<'a, TaskListResponse> { self.inner.list(p) }
    fn insert_if_absent<'a>(&'a self, t: &'a Task) -> F<'a, bool> { self.inner.insert_if_absent(t) }
    fn delete<'a>(&'a self, id: &'a TaskId) -> F<'a, ()> { self.inner.delete(id) }
    fn count<'a>(&'a self) -> F<'a, u64> {
        let down = self.down.load(Ordering::SeqCst);
        Box::pin(async move { if down { Err(A2aError::internal("db down")) } else { Ok(0) } })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn health_and_ready_per_surface() {
    let down = Arc::new(AtomicBool::new(true));
    let (agent, _) = CtlAgent::new();
    let h = Arc::new(
        RequestHandlerBuilder::new(agent)
            .with_task_store(FlakyStore { inner: InMemoryTaskStore::new(), down: down.clone() })
            .build()
            .unwrap(),
    );
    let (p1, p2, p3) = (port(), port(), port());
    serve_with_addr(format!("127.0.0.1:{p1}"), JsonRpcDispatcher::new(h.clone())).await.unwrap();
    serve_with_addr(format!("127.0.0.1:{p2}"), RestDispatcher::new(h.clone())).await.unwrap();
    let app = A2aRouter::new(h.clone()).into_router();
    let l = tokio::net::TcpListener::bind(("127.0.0.1", p3)).await.unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let mut rows = vec![];
    for (label, p) in [("JsonRpcDispatcher", p1), ("RestDispatcher", p2), ("A2aRouter", p3)] {
        for path in ["/health", "/ready"] {
            let r = reqwest::get(format!("http://127.0.0.1:{p}{path}")).await.unwrap();
            let s = r.status().as_u16();
            let b = r.text().await.unwrap();
            println!("store DOWN: {label} GET {path} -> {s} {}", &b[..b.len().min(100)]);
            rows.push((label, path, s));
        }
    }
    let ready = |l: &str| rows.iter().find(|r| r.0 == l && r.1 == "/ready").unwrap().2;
    let health = |l: &str| rows.iter().find(|r| r.0 == l && r.1 == "/health").unwrap().2;
    let mut fails = vec![];
    for l in ["JsonRpcDispatcher", "RestDispatcher", "A2aRouter"] {
        if health(l) != 200 { fails.push(format!("{l} /health={}", health(l))); }
        if ready(l) != 503 { fails.push(format!("{l} /ready={} with store down (expected 503)", ready(l))); }
    }
    println!("FAILURES: {fails:?}");
    assert!(fails.is_empty(), "{fails:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn body_limit_content_type_cors() {
    let (agent, _) = CtlAgent::new();
    let h = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());
    let (p1, p2, p3, p4) = (port(), port(), port(), port());
    let small = DispatchConfig::default().with_max_request_body_size(2048);
    serve_with_addr(format!("127.0.0.1:{p1}"), JsonRpcDispatcher::with_config(h.clone(), small.clone())).await.unwrap();
    serve_with_addr(format!("127.0.0.1:{p2}"), RestDispatcher::with_config(h.clone(), small)).await.unwrap();
    serve_with_addr(
        format!("127.0.0.1:{p3}"),
        JsonRpcDispatcher::new(h.clone()).with_cors(CorsConfig::new("https://app.example")),
    )
    .await
    .unwrap();
    serve_with_addr(format!("127.0.0.1:{p4}"), JsonRpcDispatcher::new(h.clone())).await.unwrap(); // default 4 MiB
    let rc = reqwest::Client::new();
    let big = json!({"jsonrpc":"2.0","id":1,"method":"SendMessage","params":send_params_json(&"x".repeat(4000))});
    let r1 = rc.post(format!("http://127.0.0.1:{p1}/")).header("content-type", "application/json").header("A2A-Version", "1.0").json(&big).send().await.unwrap();
    let s1 = r1.status().as_u16();
    let b1 = r1.text().await.unwrap();
    let r2 = rc.post(format!("http://127.0.0.1:{p2}/message:send")).header("content-type", "application/json").header("A2A-Version", "1.0").json(&big["params"]).send().await.unwrap();
    let s2 = r2.status().as_u16();
    let huge = json!({"jsonrpc":"2.0","id":1,"method":"SendMessage","params":send_params_json(&"x".repeat(5 * 1024 * 1024))});
    // reqwest gets EPIPE when the server answers early and closes; use curl to read the answer.
    let f = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(f.path(), serde_json::to_vec(&huge).unwrap()).unwrap();
    let out = std::process::Command::new("curl")
        .args(["-s", "-o", "-", "-w", "\n%{http_code}", "-H", "content-type: application/json", "-H", "A2A-Version: 1.0",
               "--data-binary", &format!("@{}", f.path().display()), &format!("http://127.0.0.1:{p4}/")])
        .output().unwrap();
    let so = String::from_utf8_lossy(&out.stdout).to_string();
    let (b4, code) = so.rsplit_once('\n').unwrap_or(("", "000"));
    let s4: u16 = code.trim().parse().unwrap_or(0);
    let b4 = b4.to_owned();
    println!("body limit 2 KiB: JSON-RPC 4 KB body -> {s1} {}", &b1[..b1.len().min(150)]);
    println!("body limit 2 KiB: REST 4 KB body -> {s2}");
    println!("default limit (4 MiB): JSON-RPC 5 MiB body -> {s4} {}", &b4[..b4.len().min(150)]);

    // Content-Type
    let ct1 = rc.post(format!("http://127.0.0.1:{p4}/")).header("content-type", "text/plain").header("A2A-Version", "1.0").body(serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"x"}})).unwrap()).send().await.unwrap();
    let cts = ct1.status().as_u16();
    let ctb = ct1.text().await.unwrap();
    let ct2 = rc.post(format!("http://127.0.0.1:{p2}/message:send")).header("content-type", "text/plain").header("A2A-Version", "1.0").body(serde_json::to_vec(&send_params_json("x")).unwrap()).send().await.unwrap();
    let ct3 = rc.post(format!("http://127.0.0.1:{p4}/")).header("A2A-Version", "1.0").body(serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"x"}})).unwrap()).send().await.unwrap();
    let ct3s = ct3.status().as_u16();
    let ct3b = ct3.text().await.unwrap();
    println!("Content-Type text/plain: JSON-RPC -> {cts} {}", &ctb[..ctb.len().min(160)]);
    println!("Content-Type text/plain: REST -> {}", ct2.status());
    println!("NO Content-Type header: JSON-RPC -> {ct3s} {}", &ct3b[..ct3b.len().min(100)]);

    // CORS
    let pre = rc
        .request(reqwest::Method::OPTIONS, format!("http://127.0.0.1:{p3}/"))
        .header("origin", "https://app.example")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type,a2a-version")
        .send()
        .await
        .unwrap();
    let pre_s = pre.status().as_u16();
    let pre_h: Vec<_> = pre.headers().iter().filter(|(k, _)| k.as_str().starts_with("access-control")).map(|(k, v)| format!("{k}: {}", v.to_str().unwrap())).collect();
    println!("CORS preflight -> {pre_s} {pre_h:#?}");
    let (_, hh, _) = jsonrpc_raw(&format!("http://127.0.0.1:{p3}"), "GetTask", json!({"id":"x"}), &[("origin", "https://app.example")]).await;
    println!("CORS actual response ACAO: {:?}", hh.get("access-control-allow-origin"));
    let pre_off = rc.request(reqwest::Method::OPTIONS, format!("http://127.0.0.1:{p4}/")).header("origin", "https://app.example").header("access-control-request-method", "POST").send().await.unwrap();
    println!("no CORS configured: preflight -> {} ACAO={:?}", pre_off.status(), pre_off.headers().get("access-control-allow-origin"));

    let v1: Value = serde_json::from_str(&b1).unwrap_or(Value::Null);
    assert!(s1 == 413 || v1["error"].is_object(), "oversized JSON-RPC refused");
    assert_eq!(s2, 413);
    assert!(s4 == 413 || b4.contains("error"));
    assert!(ctb.contains("-32005") || cts == 415);
    assert_eq!(ct2.status().as_u16(), 400, "0.14 deliberately maps ContentTypeNotSupported to 400 (see rest/mod.rs:136)");
    assert!(pre_s == 200 || pre_s == 204);
    assert!(pre_h.iter().any(|h| h.starts_with("access-control-allow-origin: https://app.example")));
    assert_eq!(hh.get("access-control-allow-origin").unwrap(), "https://app.example");
}

#[tokio::test(flavor = "multi_thread")]
async fn executor_timeout_default_and_custom() {
    let (agent, _) = CtlAgent::new();
    let b = RequestHandlerBuilder::new(agent);
    let dbg = format!("{b:?}");
    let default_line = dbg.split(", ").find(|s| s.contains("executor_timeout")).unwrap_or("?").to_owned();
    println!("builder default: {default_line}");
    assert!(default_line.contains("3600s"));

    let (agent, probe) = CtlAgent::new();
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_executor_timeout(Duration::from_secs(1)).build().unwrap());
    let p = port();
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();
    let t0 = std::time::Instant::now();
    let t = task_of(c.send_message(msg(&uid(), "hang:5000")).await.unwrap());
    let el = t0.elapsed();
    let reason = t.status.message.as_ref().and_then(|m| m.text()).map(str::to_owned);
    println!("with_executor_timeout(1s), hang:5000 -> state={:?} after {el:?} reason={reason:?} finished_count={}", t.status.state, probe.finished.load(Ordering::SeqCst));
    assert_eq!(t.status.state, TaskState::Failed);
    assert!(el < Duration::from_secs(3));

    // Opt-out compiles/builds
    let (agent, _) = CtlAgent::new();
    let b = RequestHandlerBuilder::new(agent).without_executor_timeout();
    println!("without_executor_timeout: {}", format!("{b:?}").split(", ").find(|s| s.contains("executor_timeout")).unwrap_or("?"));
}
