// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `jsonrpc_auth_before_params.rs` for the other bindings, and for the
//! checks the handler made before admission.
//!
//! HTTP+JSON (both of its dispatchers), gRPC and WebSocket read a request's
//! params before the interceptor chain saw the call, so a caller with no
//! credential and a malformed request was answered `400`, `INVALID_ARGUMENT`
//! or `-32602` naming the field it got wrong, where the same caller with a
//! well-formed request was refused. `ListTasks` checked `pageSize` and
//! `statusTimestampAfter`, and `CreateTaskPushNotificationConfig` its
//! `taskId`, before admission on every binding. Spec §7.4: a server "MUST
//! authenticate every incoming request"; §3.3.2: it "MUST reject requests
//! with invalid or missing authentication credentials". A check that only
//! runs on well-formed requests is not that, and it serves the parameter
//! schema to anyone who asks.
//!
//! Each binding's file sends every malformed request three ways: with no
//! credential and with a wrong one, each refused as that binding refuses a
//! caller, and with the right one, answered the validation error it always
//! got. The counter shows the credential was checked on every call.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{
    RequestHandler, RequestHandlerBuilder, ServerInterceptor, agent_executor,
};
use a2a_protocol_types::error::{A2aError, A2aResult};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use serde_json::{Value, json};

#[cfg(feature = "grpc")]
mod grpc;
mod http_json;
#[cfg(feature = "websocket")]
mod websocket;

struct Exec;
agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

const CHALLENGE: &str = "Bearer realm=\"a2a\"";

/// Admits `Bearer good` and refuses anything else, counting the calls whose
/// credential it checked.
struct TokenGate(Arc<AtomicUsize>);

impl ServerInterceptor for TokenGate {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let admitted =
            ctx.http_headers().get("authorization").map(String::as_str) == Some("Bearer good");
        Box::pin(async move {
            if admitted {
                Ok(())
            } else {
                Err(A2aError::unauthenticated("not authenticated", CHALLENGE))
            }
        })
    }
    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
    fn authenticates(&self) -> bool {
        true
    }
}

/// A handler behind [`TokenGate`], and the gate's count.
fn gated() -> (Arc<RequestHandler>, Arc<AtomicUsize>) {
    let checked = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(Exec)
        .without_agent_card()
        .with_interceptor(TokenGate(Arc::clone(&checked)))
        .build()
        .expect("handler");
    (Arc::new(handler), checked)
}

/// The tokens each request is sent with: none, a wrong one, the right one.
const TOKENS: [Option<&str>; 3] = [None, Some("bad"), Some("good")];

/// Every case a test got wrong, so that a failure lists them all rather
/// than the first.
#[derive(Default)]
struct Misses(Vec<String>);

impl Misses {
    fn check(&mut self, right: bool, what: impl FnOnce() -> String) {
        if !right {
            self.0.push(what());
        }
    }

    fn assert_none(self) {
        assert!(
            self.0.is_empty(),
            "{} case(s) answered wrongly:\n{}",
            self.0.len(),
            self.0.join("\n")
        );
    }
}

/// Whether an HTTP+JSON answer is the binding's refusal of a caller: `401`
/// with the challenge and an `UNAUTHENTICATED` body.
fn http_json_refusal(a: &Answer) -> bool {
    a.status == 401
        && a.challenge.as_deref() == Some(CHALLENGE)
        && a.body["error"]["status"] == "UNAUTHENTICATED"
}

/// The answer to one HTTP request: status, `WWW-Authenticate`, JSON body.
struct Answer {
    status: u16,
    challenge: Option<String>,
    body: Value,
}

async fn http(
    addr: std::net::SocketAddr,
    (method, path, body): (&str, &str, Option<&str>),
    token: Option<&str>,
) -> Answer {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let mut req = hyper::Request::builder()
        .method(method)
        .uri(format!("http://{addr}{path}"))
        .header("a2a-version", "1.0");
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let body = Full::new(Bytes::from(body.unwrap_or_default().to_owned()));
    let resp = client.request(req.body(body).unwrap()).await.expect("http");
    let status = resp.status().as_u16();
    let challenge = resp
        .headers()
        .get(hyper::header::WWW_AUTHENTICATE)
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = resp.collect().await.expect("body").to_bytes();
    Answer {
        status,
        challenge,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

/// The JSON-RPC calls that reach the handler with params that parse, and
/// that the handler refuses: each with the field it names.
fn handler_refused_calls() -> Vec<(Value, &'static str)> {
    [
        ("ListTasks", json!({"pageSize": 0}), "pageSize"),
        ("ListTasks", json!({"pageSize": 150}), "pageSize"),
        (
            "ListTasks",
            json!({"statusTimestampAfter": "yesterday"}),
            "statusTimestampAfter",
        ),
        (
            "CreateTaskPushNotificationConfig",
            json!({"url": "https://hook.example/a"}),
            "taskId",
        ),
    ]
    .into_iter()
    .map(|(method, params, field)| {
        let call = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (call, field)
    })
    .collect()
}

/// `ListTasks`' `pageSize` and `statusTimestampAfter`, and the `taskId` of
/// `CreateTaskPushNotificationConfig`, are checked after admission: refused
/// `401` with no credential or a wrong one, `-32602` naming the field with
/// the right one.
#[tokio::test]
async fn jsonrpc_handler_checks_run_after_admission() {
    let (handler, checked) = gated();
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler))
        .await
        .expect("serve");
    let calls = handler_refused_calls();
    let mut misses = Misses::default();
    for (call, field) in &calls {
        let body = call.to_string();
        for token in TOKENS {
            let a = http(addr, ("POST", "/", Some(&body)), token).await;
            let right = if token == Some("good") {
                a.status == 200
                    && a.body["error"]["code"] == -32602
                    && a.body.to_string().contains(field)
            } else {
                a.status == 401
                    && a.challenge.as_deref() == Some(CHALLENGE)
                    && a.body["error"]["code"] == -32600
            };
            misses.check(right, || {
                format!("{} with {token:?}: {} {}", call["params"], a.status, a.body)
            });
        }
    }
    misses.assert_none();
    assert_eq!(checked.load(Ordering::SeqCst), 3 * calls.len());
}

/// The same checks on the HTTP+JSON binding's own requests.
#[tokio::test]
async fn rest_handler_checks_run_after_admission() {
    let (handler, checked) = gated();
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler))
        .await
        .expect("serve");
    let requests = [
        ("GET", "/tasks?pageSize=0", None, "pageSize"),
        ("GET", "/tasks?pageSize=150", None, "pageSize"),
        (
            "GET",
            "/tasks?statusTimestampAfter=yesterday",
            None,
            "statusTimestampAfter",
        ),
        (
            "POST",
            "/tasks//pushNotificationConfigs",
            Some(r#"{"url": "https://hook.example/a"}"#),
            "taskId",
        ),
    ];
    let mut misses = Misses::default();
    for (method, path, body, field) in requests {
        for token in TOKENS {
            let a = http(addr, (method, path, body), token).await;
            let right = if token == Some("good") {
                a.status == 400 && a.body.to_string().contains(field)
            } else {
                http_json_refusal(&a)
            };
            misses.check(right, || {
                format!("{method} {path} with {token:?}: {} {}", a.status, a.body)
            });
        }
    }
    misses.assert_none();
    assert_eq!(checked.load(Ordering::SeqCst), 3 * requests.len());
}
