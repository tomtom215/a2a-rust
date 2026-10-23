// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! HTTP+JSON: `RestDispatcher` and the axum router, which serve the same
//! binding and so answer alike.

use std::sync::atomic::Ordering;

use a2a_protocol_server::dispatch::RestDispatcher;
use a2a_protocol_server::serve::serve_with_addr;

use super::{Misses, TOKENS, gated, http, http_json_refusal};

/// One request per way a request can fail to be read: a body that is not
/// JSON, a body of the wrong shape (on each route that reads one, the two
/// streaming ones among them), and a query value that does not parse.
const MALFORMED: [(&str, &str, Option<&str>); 9] = [
    ("POST", "/message:send", Some("{")),
    (
        "POST",
        "/message:send",
        Some(r#"{"message": "not a message"}"#),
    ),
    ("POST", "/message:stream", Some(r#"{"message": 7}"#)),
    ("GET", "/tasks/t1?historyLength=many", None),
    ("GET", "/tasks?pageSize=ten", None),
    ("POST", "/tasks/t1:cancel", Some("{")),
    ("POST", "/tasks/t1:subscribe", Some(r#"{"tenant": 7}"#)),
    (
        "POST",
        "/tasks/t1/pushNotificationConfigs",
        Some(r#"{"url": 7}"#),
    ),
    ("GET", "/tasks/t1/pushNotificationConfigs?pageSize=x", None),
];

async fn assert_refused_before_read(addr: std::net::SocketAddr) {
    let mut misses = Misses::default();
    for request in MALFORMED {
        for token in TOKENS {
            let a = http(addr, request, token).await;
            let right = if token == Some("good") {
                a.status == 400 && a.body["error"]["status"] == "INVALID_ARGUMENT"
            } else {
                http_json_refusal(&a)
            };
            misses.check(right, || {
                format!("{request:?} with {token:?}: {} {}", a.status, a.body)
            });
        }
    }
    misses.assert_none();
}

#[tokio::test]
async fn rest_malformed_request_is_refused_401_without_credential() {
    let (handler, checked) = gated();
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler))
        .await
        .expect("serve");
    assert_refused_before_read(addr).await;
    assert_eq!(checked.load(Ordering::SeqCst), 3 * MALFORMED.len());
}

#[cfg(feature = "axum")]
#[tokio::test]
async fn axum_malformed_request_is_refused_401_without_credential() {
    let (handler, checked) = gated();
    let app = a2a_protocol_server::dispatch::axum_adapter::A2aRouter::new(handler).into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await });
    assert_refused_before_read(addr).await;
    assert_eq!(checked.load(Ordering::SeqCst), 3 * MALFORMED.len());
}

/// What still comes before admission refuses a well-formed request alike,
/// and says nothing about its fields: an unsupported `A2A-Version` (§3.6.2,
/// the semantics the request is read with) and a body over the size cap
/// (§13.4). Neither reaches the chain.
#[tokio::test]
async fn rest_version_and_size_refusals_precede_admission() {
    let (handler, checked) = gated();
    let config =
        a2a_protocol_server::dispatch::DispatchConfig::default().with_max_request_body_size(64);
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::with_config(handler, config))
        .await
        .expect("serve");
    let big = format!(r#"{{"pad": "{}"}}"#, "x".repeat(128));
    let a = http(addr, ("POST", "/message:send", Some(&big)), None).await;
    assert_eq!(a.status, 413, "{}", a.body);
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<http_body_util::Full<hyper::body::Bytes>>();
    let req = hyper::Request::builder()
        .uri(format!("http://{addr}/tasks?pageSize=ten"))
        .header("a2a-version", "9.0")
        .body(http_body_util::Full::default())
        .unwrap();
    let resp = client.request(req).await.expect("http");
    assert_eq!(resp.status().as_u16(), 400);
    assert_eq!(checked.load(Ordering::SeqCst), 0);
}
