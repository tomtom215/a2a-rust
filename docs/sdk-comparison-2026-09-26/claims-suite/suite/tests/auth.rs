// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: Bearer / API-key / JWT auth interceptors: HTTP 401 + WWW-Authenticate
//! on JSON-RPC and REST, gRPC UNAUTHENTICATED, WebSocket -32600.
//! Ports 7900-7949.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::auth::jwt::{Jwks, JwtAuthInterceptor, JwtValidator};
use a2a_protocol_sdk::server::{GrpcConfig, GrpcDispatcher, WebSocketDispatcher};
use base64::Engine;
use claims_suite::common::*;
use serde_json::json;

fn port() -> u16 {
    port_in(7900, 50)
}

fn hs256(secret: &[u8], claims: serde_json::Value) -> String {
    let b = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let h = b.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let p = b.encode(serde_json::to_vec(&claims).unwrap());
    let input = format!("{h}.{p}");
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret);
    let sig = ring::hmac::sign(&key, input.as_bytes());
    format!("{input}.{}", b.encode(sig.as_ref()))
}

struct Surfaces {
    jr: u16,
    rest: u16,
    grpc: u16,
    ws: u16,
}

async fn serve_all(b: RequestHandlerBuilder) -> Surfaces {
    let h = Arc::new(b.build().unwrap());
    let s = Surfaces { jr: port(), rest: port(), grpc: port(), ws: port() };
    serve_with_addr(format!("127.0.0.1:{}", s.jr), JsonRpcDispatcher::new(h.clone())).await.unwrap();
    serve_with_addr(format!("127.0.0.1:{}", s.rest), RestDispatcher::new(h.clone())).await.unwrap();
    GrpcDispatcher::new(h.clone(), GrpcConfig::default()).serve_with_addr(format!("127.0.0.1:{}", s.grpc)).await.unwrap();
    Arc::new(WebSocketDispatcher::new(h)).serve_with_addr(format!("127.0.0.1:{}", s.ws)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    s
}

async fn probe(label: &str, s: &Surfaces, hdr: Option<(&str, &str)>) -> Vec<String> {
    let mut out = vec![];
    let hs: Vec<(&str, &str)> = hdr.into_iter().collect();
    // JSON-RPC
    let (st, h, b) = jsonrpc_raw(&format!("http://127.0.0.1:{}", s.jr), "GetTask", json!({"id":"x"}), &hs).await;
    out.push(format!("{label} JSON-RPC: {st} www-authenticate={:?} body={}", h.get("www-authenticate"), &b[..b.len().min(110)]));
    // REST
    let mut rb = reqwest::Client::new().get(format!("http://127.0.0.1:{}/tasks/x", s.rest)).header("A2A-Version", "1.0");
    for (k, v) in &hs { rb = rb.header(*k, *v); }
    let r = rb.send().await.unwrap();
    let st = r.status().as_u16();
    let w = r.headers().get("www-authenticate").cloned();
    let b = r.text().await.unwrap();
    out.push(format!("{label} REST: {st} www-authenticate={w:?} body={}", &b[..b.len().min(110)]));
    // gRPC (raw HTTP/2 via curl, read grpc-status trailer). Empty GetTaskRequest message.
    let mut args = vec![
        "-s".to_string(), "-v".into(), "--http2-prior-knowledge".into(), "-X".into(), "POST".into(),
        "-H".into(), "content-type: application/grpc".into(), "-H".into(), "te: trailers".into(),
        "-H".into(), "a2a-version: 1.0".into(),
        "--data-binary".into(), "@-".into(),
        format!("http://127.0.0.1:{}/lf.a2a.v1.A2AService/GetTask", s.grpc),
    ];
    if let Some((k, v)) = hdr { args.insert(0, format!("{k}: {v}")); args.insert(0, "-H".into()); }
    let mut child = std::process::Command::new("curl").args(&args)
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    {
        use std::io::Write;
        // gRPC frame: flag 0, length 3, protobuf field 1 (id) = "x"
        child.stdin.take().unwrap().write_all(&[0, 0, 0, 0, 3, 0x0a, 1, b'x']).unwrap();
    }
    let o = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    let status: Vec<_> = err.lines().filter(|l| l.contains("grpc-status") || l.contains("grpc-message") || l.contains("HTTP/2 ")).map(|l| l.trim().to_owned()).collect();
    out.push(format!("{label} gRPC: {status:?}"));
    // WebSocket (credentials at upgrade)
    let mut extra = std::collections::HashMap::new();
    if let Some((k, v)) = hdr { extra.insert(k.to_string(), v.to_string()); }
    let ws = a2a_protocol_sdk::client::WebSocketTransport::connect_with_options(format!("ws://127.0.0.1:{}", s.ws), Duration::from_secs(5), &extra).await;
    let wsr = match ws {
        Err(e) => format!("connect error: {e}"),
        Ok(t) => {
            let c = ClientBuilder::new(format!("ws://127.0.0.1:{}", s.ws)).with_custom_transport(t).build().unwrap();
            match c.get_task(TaskQueryParams::new("x")).await { Ok(_) => "ok".into(), Err(e) => format!("{e:?}") }
        }
    };
    out.push(format!("{label} WebSocket: {}", &wsr[..wsr.len().min(160)]));
    for l in &out { println!("{l}"); }
    out
}

fn assert_401(out: &[String], scheme: &str) {
    assert!(out[0].contains(": 401 www-authenticate=Some(") && out[0].contains(scheme), "JSON-RPC: {}", out[0]);
    assert!(out[1].contains(": 401 www-authenticate=Some(") && out[1].contains(scheme), "REST: {}", out[1]);
    assert!(out[2].contains("grpc-status: 16"), "gRPC UNAUTHENTICATED: {}", out[2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn bearer_static() {
    let (agent, _) = CtlAgent::new();
    let s = serve_all(RequestHandlerBuilder::new(agent).with_interceptor(BearerTokenAuthInterceptor::new(["good-token"]))).await;
    let none = probe("bearer/no-cred", &s, None).await;
    let bad = probe("bearer/wrong", &s, Some(("authorization", "Bearer nope"))).await;
    let good = probe("bearer/good", &s, Some(("authorization", "Bearer good-token"))).await;
    assert_401(&none, "Bearer");
    assert_401(&bad, "Bearer");
    assert!(good[0].contains("-32001"), "authorized request reaches handler (TaskNotFound)");
}

#[tokio::test(flavor = "multi_thread")]
async fn api_key() {
    let (agent, _) = CtlAgent::new();
    let s = serve_all(RequestHandlerBuilder::new(agent).with_interceptor(ApiKeyAuthInterceptor::new(["k1"]))).await;
    let none = probe("apikey/no-cred", &s, None).await;
    let bad = probe("apikey/wrong", &s, Some(("x-api-key", "k2"))).await;
    let good = probe("apikey/good", &s, Some(("x-api-key", "k1"))).await;
    assert_401(&none, "ApiKey");
    assert_401(&bad, "ApiKey");
    assert!(good[0].contains("-32001"));
}

#[tokio::test(flavor = "multi_thread")]
async fn jwt_hs256() {
    let (agent, _) = CtlAgent::new();
    let v = JwtValidator::new().with_hs256_secret(b"s3cret".to_vec()).with_issuer("iss").with_audience("aud");
    let s = serve_all(RequestHandlerBuilder::new(agent).with_interceptor(JwtAuthInterceptor::new(v, Jwks::new()))).await;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let good = hs256(b"s3cret", json!({"sub":"alice","iss":"iss","aud":"aud","exp": now + 300}));
    let expired = hs256(b"s3cret", json!({"sub":"alice","iss":"iss","aud":"aud","exp": now - 3600}));
    let wrongkey = hs256(b"other", json!({"sub":"alice","iss":"iss","aud":"aud","exp": now + 300}));
    let none = probe("jwt/no-cred", &s, None).await;
    let exp = probe("jwt/expired", &s, Some(("authorization", &format!("Bearer {expired}")))).await;
    let wk = probe("jwt/wrong-key", &s, Some(("authorization", &format!("Bearer {wrongkey}")))).await;
    let ok = probe("jwt/good", &s, Some(("authorization", &format!("Bearer {good}")))).await;
    assert_401(&none, "Bearer");
    assert_401(&exp, "Bearer");
    assert_401(&wk, "Bearer");
    assert!(ok[0].contains("-32001"));
}
