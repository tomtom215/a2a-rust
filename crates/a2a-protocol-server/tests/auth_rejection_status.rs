// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A refused credential answers with each HTTP binding's own status (N36).
//!
//! Every auth interceptor used to refuse with `InvalidRequest`, which both
//! HTTP bindings answer as `400` — a malformed request, not a refused
//! credential, so a client could not tell to refresh its token (an OAuth
//! client discards a cached token on `401`, and on nothing else). Nothing
//! pinned the status, which is how it went unnoticed; ACTS SEC-AUTH-006 and
//! SEC-EXTCARD-001/002/004 found it.
//!
//! Each case has a control: the same call with the valid credential is
//! served, so the refusal is attributable to the credential. gRPC is pinned
//! in the SDK crate, which has a client (`tests/auth_rejection_grpc.rs`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{
    ApiKeyAuthInterceptor, BearerTokenAuthInterceptor, RequestHandler, RequestHandlerBuilder,
    ServerInterceptor, agent_executor,
};
use a2a_protocol_types::error::{A2aError, A2aResult};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;

struct Exec;
agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

fn handler(interceptor: impl ServerInterceptor + 'static) -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Exec)
            .with_interceptor(interceptor)
            .build()
            .expect("handler"),
    )
}

/// Refuses every call as an authenticated caller without permission.
struct DenyAll;

impl ServerInterceptor for DenyAll {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Err(A2aError::permission_denied("not permitted")) })
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

struct Reply {
    status: u16,
    challenge: Option<String>,
    body: serde_json::Value,
}

async fn call(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    headers: &[(&str, &str)],
) -> Reply {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let mut req = hyper::Request::builder()
        .method(method)
        .uri(format!("http://{addr}{path}"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let body = body.map_or_else(Bytes::new, |b| Bytes::from(b.to_string()));
    let resp = client
        .request(req.body(Full::new(body)).unwrap())
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let challenge = resp
        .headers()
        .get(hyper::header::WWW_AUTHENTICATE)
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        challenge,
        body: serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    }
}

fn get_task() -> serde_json::Value {
    serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "GetTask", "params": {"id": "nope"}})
}

const BEARER: &str = "Bearer realm=\"a2a\"";

#[tokio::test]
async fn rest_answers_a_missing_bearer_token_401_with_its_challenge() {
    let h = handler(BearerTokenAuthInterceptor::new(["good"]));
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(h))
        .await
        .unwrap();

    let refused = call(addr, "GET", "/tasks/nope", None, &[]).await;
    assert_eq!(refused.status, 401, "{}", refused.body);
    assert_eq!(refused.challenge.as_deref(), Some(BEARER));
    assert_eq!(refused.body["error"]["code"], 401);
    assert_eq!(refused.body["error"]["status"], "UNAUTHENTICATED");

    let wrong = call(
        addr,
        "GET",
        "/tasks/nope",
        None,
        &[("authorization", "Bearer bad")],
    )
    .await;
    assert_eq!(
        wrong.status, 401,
        "a wrong token is refused like a missing one"
    );
    assert_eq!(wrong.challenge.as_deref(), Some(BEARER));

    let served = call(
        addr,
        "GET",
        "/tasks/nope",
        None,
        &[("authorization", "Bearer good")],
    )
    .await;
    assert_eq!(
        served.status, 404,
        "the control: the valid token reaches the handler"
    );
    assert_eq!(served.challenge, None);
}

#[tokio::test]
async fn json_rpc_answers_a_missing_bearer_token_401_with_the_body_unchanged() {
    let h = handler(BearerTokenAuthInterceptor::new(["good"]));
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(h))
        .await
        .unwrap();

    let refused = call(addr, "POST", "/", Some(get_task()), &[]).await;
    assert_eq!(refused.status, 401, "{}", refused.body);
    assert_eq!(refused.challenge.as_deref(), Some(BEARER));
    assert_eq!(
        refused.body["error"]["code"], -32600,
        "the JSON-RPC body is as it was"
    );
    assert_eq!(refused.body["id"], 1);

    let served = call(
        addr,
        "POST",
        "/",
        Some(get_task()),
        &[("authorization", "Bearer good")],
    )
    .await;
    assert_eq!(
        served.status, 200,
        "the control: an ordinary JSON-RPC error is still 200"
    );
    assert_eq!(served.body["error"]["code"], -32001);
}

#[tokio::test]
async fn an_api_key_refusal_names_its_header_in_the_challenge() {
    let h = handler(ApiKeyAuthInterceptor::new(["k"]));
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(h))
        .await
        .unwrap();

    let refused = call(addr, "GET", "/tasks/nope", None, &[]).await;
    assert_eq!(refused.status, 401);
    assert_eq!(
        refused.challenge.as_deref(),
        Some("ApiKey header=\"x-api-key\"")
    );

    let served = call(addr, "GET", "/tasks/nope", None, &[("x-api-key", "k")]).await;
    assert_eq!(served.status, 404);
}

#[tokio::test]
async fn permission_denied_answers_403_without_a_challenge_on_both_bindings() {
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler(DenyAll)))
        .await
        .unwrap();
    let refused = call(rest, "GET", "/tasks/nope", None, &[]).await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.challenge, None);
    assert_eq!(refused.body["error"]["status"], "PERMISSION_DENIED");

    let jsonrpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler(DenyAll)))
        .await
        .unwrap();
    let refused = call(jsonrpc, "POST", "/", Some(get_task()), &[]).await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert_eq!(refused.challenge, None);
    assert_eq!(refused.body["error"]["code"], -32600);
}

#[cfg(feature = "axum")]
#[tokio::test]
async fn the_axum_adapter_answers_a_missing_bearer_token_401_with_its_challenge() {
    use a2a_protocol_server::dispatch::axum_adapter::A2aRouter;

    let app = A2aRouter::new(handler(BearerTokenAuthInterceptor::new(["good"]))).into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });

    let refused = call(addr, "GET", "/tasks/nope", None, &[]).await;
    assert_eq!(refused.status, 401);
    assert_eq!(refused.challenge.as_deref(), Some(BEARER));

    let served = call(
        addr,
        "GET",
        "/tasks/nope",
        None,
        &[("authorization", "Bearer good")],
    )
    .await;
    assert_eq!(served.status, 404);
}
