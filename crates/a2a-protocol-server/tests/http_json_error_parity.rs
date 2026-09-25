// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The two HTTP+JSON dispatchers answer the same failure identically (N37).
//!
//! The axum adapter answered errors as `{"error": "<text>"}`, where
//! `RestDispatcher` answers the AIP-193 `google.rpc.Status` spec §11.6 names:
//! the same request got a different error shape depending on which of this
//! crate's two HTTP+JSON dispatchers served it, and the adapter's carried no
//! `code`, `status` or `ErrorInfo`. The adapter now answers through the REST
//! dispatcher's builders; this pins that they agree, byte for byte.
#![cfg(feature = "axum")]

use std::sync::Arc;

use a2a_protocol_server::dispatch::RestDispatcher;
use a2a_protocol_server::dispatch::axum_adapter::A2aRouter;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{
    BearerTokenAuthInterceptor, RequestHandler, RequestHandlerBuilder, agent_executor,
};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;

struct Exec;
agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

fn handler() -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Exec)
            .with_interceptor(BearerTokenAuthInterceptor::new(["good"]))
            .build()
            .expect("handler"),
    )
}

async fn call(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: &'static str,
    token: Option<&str>,
) -> (u16, Option<String>, serde_json::Value) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let mut req = hyper::Request::builder()
        .method(method)
        .uri(format!("http://{addr}{path}"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let resp = client
        .request(
            req.body(Full::new(Bytes::from_static(body.as_bytes())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let challenge = resp
        .headers()
        .get(hyper::header::WWW_AUTHENTICATE)
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        challenge,
        serde_json::from_slice(&bytes).unwrap_or_default(),
    )
}

#[tokio::test]
async fn rest_and_axum_answer_the_same_failures_identically() {
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler()))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let axum_addr = listener.local_addr().unwrap();
    let app = A2aRouter::new(handler()).into_router();
    tokio::spawn(async move { axum::serve(listener, app).await });

    let cases: [(&str, &str, &str, Option<&str>, u16); 3] = [
        // A task that does not exist: an A2A error, with its ErrorInfo.
        ("GET", "/tasks/nope", "", Some("good"), 404),
        // A body that is not JSON.
        ("POST", "/message:send", "{", Some("good"), 400),
        // No credential: 401 with a challenge (N36).
        ("GET", "/tasks/nope", "", None, 401),
    ];
    for (method, path, body, token, status) in cases {
        let r = call(rest, method, path, body, token).await;
        let a = call(axum_addr, method, path, body, token).await;
        assert_eq!(r.0, status, "{method} {path}: {}", r.2);
        assert_eq!(
            a, r,
            "{method} {path}: the adapter disagrees with RestDispatcher"
        );
        assert!(r.2["error"]["code"].is_number(), "AIP-193 `code`: {}", r.2);
        assert!(
            r.2["error"]["status"].is_string(),
            "AIP-193 `status`: {}",
            r.2
        );
    }
}
