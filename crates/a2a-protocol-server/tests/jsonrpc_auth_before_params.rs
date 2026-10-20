// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! JSON-RPC read a call's `params` before the interceptor chain saw the
//! call, so a caller with no credential and a malformed body was answered
//! `-32602` naming the field it got wrong, where the same caller with a
//! well-formed body was answered `401`. The parameter schema was served to
//! anyone who asked, and a credential check that only runs on well-formed
//! calls is not the check on every operation spec §13.1 asks for.
//!
//! A call whose params do not parse is now admitted like any other first:
//! an unauthenticated one gets the interceptor's refusal, and one the chain
//! admits gets the `-32602` it always got. The credential is checked on the
//! way, which the counter shows.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{RequestHandlerBuilder, ServerInterceptor, agent_executor};
use a2a_protocol_types::error::{A2aError, A2aResult};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use serde_json::{Value, json};

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

async fn start() -> (std::net::SocketAddr, Arc<AtomicUsize>) {
    let checked = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(Exec)
        .without_agent_card()
        .with_interceptor(TokenGate(Arc::clone(&checked)))
        .build()
        .expect("handler");
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::new(handler)))
        .await
        .expect("serve");
    (addr, checked)
}

async fn post(
    addr: std::net::SocketAddr,
    body: &Value,
    token: Option<&str>,
) -> (u16, Option<String>, Value) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let mut req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0");
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let resp = client
        .request(req.body(Full::new(Bytes::from(body.to_string()))).unwrap())
        .await
        .expect("http");
    let status = resp.status().as_u16();
    let challenge = resp
        .headers()
        .get(hyper::header::WWW_AUTHENTICATE)
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = resp.collect().await.expect("body").to_bytes();
    (
        status,
        challenge,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// One call per method whose params cannot be read, the two streaming
/// methods among them: each has a field of the wrong type or a required
/// one missing.
fn malformed_calls() -> Vec<Value> {
    [
        ("SendMessage", json!({"message": "not a message"})),
        ("SendStreamingMessage", json!({"message": 7})),
        ("SubscribeToTask", json!({"id": 7})),
        ("GetTask", json!({"id": 7})),
        ("ListTasks", json!({"pageSize": "ten"})),
        ("CancelTask", json!({})),
        ("CreateTaskPushNotificationConfig", json!({"taskId": []})),
        ("GetTaskPushNotificationConfig", json!({"taskId": 7})),
        ("ListTaskPushNotificationConfigs", json!({"taskId": {}})),
        ("DeleteTaskPushNotificationConfig", json!({"id": false})),
    ]
    .into_iter()
    .map(|(method, params)| json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))
    .collect()
}

#[tokio::test]
async fn jsonrpc_malformed_params_without_credential_is_refused_401() {
    let (addr, checked) = start().await;
    for call in malformed_calls() {
        let method = &call["method"];
        let (status, challenge, body) = post(addr, &call, None).await;
        assert_eq!(status, 401, "{method}: {body}");
        assert_eq!(challenge.as_deref(), Some(CHALLENGE), "{method}");
        assert_eq!(body["error"]["code"], -32600, "{method}: {body}");
        let (status, _, body) = post(addr, &call, Some("bad")).await;
        assert_eq!(status, 401, "{method} with a wrong token: {body}");
    }
    assert_eq!(checked.load(Ordering::SeqCst), 2 * malformed_calls().len());

    // In a batch the answer is `200` (one response answers many calls), and
    // the refused call's entry is the refusal, not `-32602`.
    let batch = Value::Array(vec![malformed_calls().swap_remove(3)]);
    let (status, _, body) = post(addr, &batch, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body[0]["error"]["code"], -32600, "{body}");
}

#[tokio::test]
async fn jsonrpc_malformed_params_with_credential_is_invalid_params() {
    let (addr, checked) = start().await;
    for call in malformed_calls() {
        let method = &call["method"];
        let (status, _, body) = post(addr, &call, Some("good")).await;
        assert_eq!(status, 200, "{method}: {body}");
        assert_eq!(body["error"]["code"], -32602, "{method}: {body}");
    }
    assert_eq!(
        checked.load(Ordering::SeqCst),
        malformed_calls().len(),
        "every malformed call's credential was checked before it was answered"
    );
}
