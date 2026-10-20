// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Features the specification leaves optional (MAY), each asserted on what
//! the server does: the behaviour is observed, so each test fails when the
//! feature is removed or broken. One binary, so the suite links once.

mod extended_card;
mod history_limit;
mod list_page_size;
mod push_retry;
mod rate_tiers;
mod rejected;
mod wire;

use std::collections::HashMap;
use std::net::SocketAddr;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;

/// A status code and the JSON body (`Null` when the body is not JSON).
pub struct Reply {
    pub status: u16,
    pub body: serde_json::Value,
}

/// One HTTP request to `addr`, with the A2A version header and `headers`.
pub async fn http(
    addr: SocketAddr,
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
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    }
}

/// The headers of a request carrying `Authorization: Bearer <token>`.
pub fn bearer(token: &str) -> HashMap<String, String> {
    HashMap::from([("authorization".to_owned(), format!("Bearer {token}"))])
}
