// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Hardening dispatch tests.
//!
//! Covers edge cases for REST content-type rejection, health and ready
//! endpoints, and path traversal protection.

use super::*;

#[tokio::test]
async fn rest_rejects_wrong_content_type_on_post() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let body = serde_json::to_vec(&make_send_params()).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/message:send"))
        .header("content-type", "text/xml")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 415, "wrong content type should return 415");
}

#[tokio::test]
async fn rest_health_endpoint_returns_ok() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/health"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse health");
    assert_eq!(value["status"], "ok");
}

#[tokio::test]
async fn rest_ready_endpoint_returns_ok() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/ready"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn rest_rejects_path_traversal() {
    let (addr, _handle) = start_rest_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("GET")
        .uri(format!("http://{addr}/tasks/../../../etc/passwd"))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 400, "path traversal should be rejected");
}
