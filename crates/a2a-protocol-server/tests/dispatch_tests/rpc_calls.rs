// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What each call records through `Metrics::on_rpc_call` (ADR 0013; audit
//! O5, O11): the method, and for a failure the status code the binding
//! answered with — including the requests the handler never sees, which no
//! other callback reports.

use std::sync::Mutex;

use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{Metrics, RpcCall};

use super::*;

/// `(system, method, status_code, error_type)` of one recorded call.
type Call = (String, Option<String>, Option<String>, Option<String>);

/// Every call recorded.
type Seen = Arc<Mutex<Vec<Call>>>;

struct Recording(Seen);

impl Metrics for Recording {
    fn on_rpc_call(&self, call: &RpcCall<'_>) {
        self.0.lock().unwrap().push((
            call.system.to_owned(),
            call.method.map(str::to_owned),
            call.status_code.map(str::to_owned),
            call.error_type.map(str::to_owned),
        ));
    }
}

fn handler(seen: &Seen) -> Arc<a2a_protocol_server::RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(SimpleExecutor)
            .with_agent_card(minimal_agent_card())
            .with_push_sender(MockPushSender)
            .with_metrics(Recording(Arc::clone(seen)))
            .build()
            .expect("build handler"),
    )
}

async fn post(addr: SocketAddr, path: &str, body: &str) -> hyper::StatusCode {
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}{path}"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .body(Full::new(Bytes::from(body.to_owned())))
        .unwrap();
    let resp = http_client().request(req).await.expect("request");
    let status = resp.status();
    let _ = resp.into_body().collect().await;
    status
}

fn failed(system: &str, method: Option<&str>, code: &str) -> Call {
    (
        system.to_owned(),
        method.map(str::to_owned),
        Some(code.to_owned()),
        Some(code.to_owned()),
    )
}

#[tokio::test]
async fn jsonrpc_records_every_refusal_with_the_code_it_answered() {
    let seen = Seen::default();
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler(&seen)))
        .await
        .expect("serve");

    // Not JSON: refused before any method is named.
    post(addr, "/", "not json at all").await;
    // A method this server does not serve.
    post(
        addr,
        "/",
        r#"{"jsonrpc":"2.0","id":1,"method":"tasks/get","params":{}}"#,
    )
    .await;
    // Nor the v0.3 spelling of one it does: refused, so `_OTHER`, not
    // `SendStreamingMessage`, which it was recorded as until 2026-10-20.
    post(
        addr,
        "/",
        r#"{"jsonrpc":"2.0","id":1,"method":"message/stream","params":{}}"#,
    )
    .await;
    // A served method with params it cannot read.
    post(
        addr,
        "/",
        r#"{"jsonrpc":"2.0","id":2,"method":"GetTask","params":{"id":7}}"#,
    )
    .await;
    // A served method on a task that does not exist.
    post(
        addr,
        "/",
        r#"{"jsonrpc":"2.0","id":3,"method":"GetTask","params":{"id":"nope"}}"#,
    )
    .await;
    // A batch: one item that is not a request, one that is.
    post(
        addr,
        "/",
        r#"[42, {"jsonrpc":"2.0","id":4,"method":"GetTask","params":{"id":"nope"}}]"#,
    )
    .await;

    let get_task = Some("lf.a2a.v1.A2AService/GetTask");
    assert_eq!(
        *seen.lock().unwrap(),
        [
            failed("jsonrpc", None, "-32700"),
            failed("jsonrpc", Some("_OTHER"), "-32601"),
            failed("jsonrpc", Some("_OTHER"), "-32601"),
            failed("jsonrpc", get_task, "-32602"),
            failed("jsonrpc", get_task, "-32001"),
            // JSON, but not a request: Invalid Request, as JSON-RPC 2.0
            // answers its own `[1]` example (N33; -32700 before 2026-09-25).
            failed("jsonrpc", None, "-32600"),
            failed("jsonrpc", get_task, "-32001"),
        ]
    );
}

/// A request refused for its `A2A-Version` is recorded once, a batch once
/// per request it answers (audit R18), and a body that is not JSON once.
#[tokio::test]
async fn jsonrpc_records_a_version_refusal_per_answer() {
    let seen = Seen::default();
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler(&seen)))
        .await
        .expect("serve");
    let request = r#"{"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"t"}}"#;
    for body in [
        request.to_owned(),
        format!("[{request},{request}]"),
        "{".to_owned(),
    ] {
        let req = hyper::Request::builder()
            .method("POST")
            .uri(format!("http://{addr}/"))
            .header("content-type", "application/json")
            .header("a2a-version", "0.3")
            .body(Full::new(Bytes::from(body)))
            .unwrap();
        let resp = http_client().request(req).await.expect("request");
        let _ = resp.into_body().collect().await;
    }
    assert_eq!(
        *seen.lock().unwrap(),
        vec![failed("jsonrpc", None, "-32009"); 4]
    );
}

#[tokio::test]
async fn rest_records_the_http_status_it_answered() {
    let seen = Seen::default();
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler(&seen)))
        .await
        .expect("serve");

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/tasks/nope"))
        .header("a2a-version", "1.0")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let resp = http_client().request(req).await.expect("request");
    assert_eq!(resp.status(), 404);
    // Refused before the handler runs: a body that is not a send request.
    assert_eq!(post(addr, "/message:send", "not json").await, 400);

    assert_eq!(*seen.lock().unwrap(), http_json_failures());
}

/// A call answered with `code` that is not a server failure: the status is
/// recorded, `error.type` is not.
fn answered(system: &str, method: Option<&str>, code: &str) -> Call {
    (
        system.to_owned(),
        method.map(str::to_owned),
        Some(code.to_owned()),
        None,
    )
}

/// A missing task, then a send refused before the handler ran: both 4xx,
/// recorded with their status and, since 2026-10-20, without `error.type`.
/// The HTTP conventions leave a server span's status unset for a 4xx and set
/// `error.type` only for a status that "indicates an error" by that rule
/// (`docs/http/http-spans.md`); these expected the status as `error.type`
/// while every non-2xx counted as a server failure.
fn http_json_failures() -> [Call; 2] {
    [
        answered("a2a_http_json", Some("lf.a2a.v1.A2AService/GetTask"), "404"),
        answered(
            "a2a_http_json",
            Some("lf.a2a.v1.A2AService/SendMessage"),
            "400",
        ),
    ]
}

#[cfg(feature = "axum")]
#[tokio::test]
async fn the_axum_router_records_the_http_status_it_answered() {
    let seen = Seen::default();
    let app =
        a2a_protocol_server::dispatch::axum_adapter::A2aRouter::new(handler(&seen)).into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await });

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/tasks/nope"))
        .header("a2a-version", "1.0")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let resp = http_client().request(req).await.expect("request");
    assert_eq!(resp.status(), 404);
    assert_eq!(post(addr, "/message:send", "not json").await, 400);

    assert_eq!(*seen.lock().unwrap(), http_json_failures());
}

/// `GET path` with `a2a-version: version`, or no version header; the status.
async fn get(addr: SocketAddr, path: &str, version: Option<&str>) -> hyper::StatusCode {
    let mut req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}{path}"));
    if let Some(version) = version {
        req = req.header("a2a-version", version);
    }
    let resp = http_client()
        .request(req.body(Full::new(Bytes::new())).unwrap())
        .await
        .expect("request");
    let status = resp.status();
    let _ = resp.into_body().collect().await;
    status
}

/// Requests refused before routing: no `A2A-Version`, one this server does
/// not speak, and a path no route serves.
async fn refuse_before_routing(addr: SocketAddr) {
    assert_eq!(get(addr, "/tasks/t", None).await, 400);
    assert_eq!(get(addr, "/tasks/t", Some("2.0")).await, 400);
    assert_eq!(get(addr, "/tasks/t/no-such-route", Some("1.0")).await, 404);
}

/// Each is one `rpc.server.call.duration` point, with the status it was
/// answered and no method — none was recorded until 2026-10-20 — and, a 4xx,
/// no `error.type` (`RpcSystem::is_server_error`).
fn refused_before_routing() -> [Call; 3] {
    [
        answered("a2a_http_json", None, "400"),
        answered("a2a_http_json", None, "400"),
        answered("a2a_http_json", None, "404"),
    ]
}

#[tokio::test]
async fn rest_records_each_request_refused_before_routing() {
    let seen = Seen::default();
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler(&seen)))
        .await
        .expect("serve");
    refuse_before_routing(addr).await;
    assert_eq!(*seen.lock().unwrap(), refused_before_routing());
}

#[cfg(feature = "axum")]
#[tokio::test]
async fn the_axum_router_records_each_request_refused_before_routing() {
    let seen = Seen::default();
    let app =
        a2a_protocol_server::dispatch::axum_adapter::A2aRouter::new(handler(&seen)).into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await });
    refuse_before_routing(addr).await;
    assert_eq!(*seen.lock().unwrap(), refused_before_routing());
}
