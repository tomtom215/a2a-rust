// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: Agent card discovery + HTTP caching (ETag/Last-Modified/304),
//! hot-reload (file polling, SIGHUP), agent card signing (signing feature).
//! Ports 7550-7599.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{A2aRouter, DynamicAgentCardHandler, HotReloadAgentCardHandler};
use a2a_protocol_sdk::types::signing::{sign_agent_card, verify_agent_card};
use claims_suite::common::*;

fn port() -> u16 {
    port_in(7550, 50)
}

fn card(url: &str) -> AgentCard {
    AgentCard::new("card-agent", "1.0.0", AgentInterface::jsonrpc(url))
        .with_description("v1")
}

async fn get(url: &str, hdrs: &[(&str, &str)]) -> (u16, reqwest::header::HeaderMap, String) {
    let c = reqwest::Client::new();
    let mut rb = c.get(url);
    for (k, v) in hdrs {
        rb = rb.header(*k, *v);
    }
    let r = rb.send().await.unwrap();
    let s = r.status().as_u16();
    let h = r.headers().clone();
    (s, h, r.text().await.unwrap())
}

/// ETag / Last-Modified / 304 on each of the three HTTP surfaces that serve
/// /.well-known/agent-card.json: JsonRpcDispatcher, RestDispatcher, A2aRouter.
#[tokio::test(flavor = "multi_thread")]
async fn card_http_caching_per_surface() {
    let (agent, _p) = CtlAgent::new();
    let p1 = port();
    let p2 = port();
    let p3 = port();
    let handler = Arc::new(
        RequestHandlerBuilder::new(agent)
            .with_agent_card(card(&format!("http://127.0.0.1:{p1}")))
            .build()
            .unwrap(),
    );
    serve_with_addr(format!("127.0.0.1:{p1}"), JsonRpcDispatcher::new(handler.clone())).await.unwrap();
    serve_with_addr(format!("127.0.0.1:{p2}"), RestDispatcher::new(handler.clone())).await.unwrap();
    let app = A2aRouter::new(handler.clone()).into_router();
    let l = tokio::net::TcpListener::bind(format!("127.0.0.1:{p3}")).await.unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Client-side resolution
    let resolved = resolve_agent_card(&format!("http://127.0.0.1:{p1}")).await.expect("resolve_agent_card");
    assert_eq!(resolved.name, "card-agent");
    println!("resolve_agent_card OK: name={} extensions={:?}", resolved.name,
        resolved.capabilities.extensions.as_ref().map(|v| v.iter().map(|e| e.uri.clone()).collect::<Vec<_>>()));

    let mut failures = vec![];
    for (label, p) in [("JsonRpcDispatcher", p1), ("RestDispatcher", p2), ("A2aRouter(axum)", p3)] {
        let url = format!("http://127.0.0.1:{p}/.well-known/agent-card.json");
        let (s, h, _b) = get(&url, &[]).await;
        let etag = h.get("etag").map(|v| v.to_str().unwrap().to_owned());
        let lm = h.get("last-modified").map(|v| v.to_str().unwrap().to_owned());
        println!("{label}: status={s} etag={etag:?} last-modified={lm:?} cache-control={:?}", h.get("cache-control"));
        assert_eq!(s, 200);
        let Some(etag) = etag else {
            failures.push(format!("{label}: no ETag header"));
            continue;
        };
        let (s304, _, b304) = get(&url, &[("if-none-match", &etag)]).await;
        println!("{label}: If-None-Match -> {s304} body_len={}", b304.len());
        if s304 != 304 {
            failures.push(format!("{label}: If-None-Match gave {s304}"));
        }
        if let Some(lm) = lm {
            let (sims, _, _) = get(&url, &[("if-modified-since", &lm)]).await;
            println!("{label}: If-Modified-Since(own value) -> {sims}");
            if sims != 304 {
                failures.push(format!("{label}: If-Modified-Since gave {sims}"));
            }
        } else {
            failures.push(format!("{label}: no Last-Modified header"));
        }
    }
    println!("FAILURES: {failures:?}");
    assert!(failures.is_empty(), "caching claim failures: {failures:?}");
}

/// Hot reload via file polling and SIGHUP. The docs say HotReloadAgentCardHandler
/// "plugs directly into DynamicAgentCardHandler"; nothing in the SDK serves a
/// DynamicAgentCardHandler, so we mount it on our own axum route (what a user must do).
#[tokio::test(flavor = "multi_thread")]
async fn card_hot_reload_poll_and_sighup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.json");
    let c1 = card("http://127.0.0.1:1");
    std::fs::write(&path, serde_json::to_vec(&c1).unwrap()).unwrap();

    let hot = HotReloadAgentCardHandler::new(c1.clone());
    let _poll = hot.spawn_poll_watcher(&path, Duration::from_millis(200));
    let dynh = Arc::new(DynamicAgentCardHandler::new(hot.clone()));

    let p = port();
    let d2 = dynh.clone();
    let app = axum::Router::new().route(
        "/.well-known/agent-card.json",
        axum::routing::get(move |headers: axum::http::HeaderMap| {
            let d = d2.clone();
            async move {
                let mut req = hyper::Request::builder().uri("/.well-known/agent-card.json");
                for (k, v) in &headers {
                    req = req.header(k, v);
                }
                let req = req.body(http_body_util::Empty::<bytes::Bytes>::new()).unwrap();
                let resp = d.handle(&req).await;
                let (parts, body) = resp.into_parts();
                axum::response::Response::from_parts(parts, axum::body::Body::new(body))
            }
        }),
    );
    let l = tokio::net::TcpListener::bind(format!("127.0.0.1:{p}")).await.unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let url = format!("http://127.0.0.1:{p}/.well-known/agent-card.json");

    let (_, h1, b1) = get(&url, &[]).await;
    let v: serde_json::Value = serde_json::from_str(&b1).unwrap();
    assert_eq!(v["description"], "v1");
    let etag1 = h1["etag"].to_str().unwrap().to_owned();
    let lm1 = h1["last-modified"].to_str().unwrap().to_owned();

    // Unchanged card, but 1.5 s later: If-Modified-Since with the value we got
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let (s_ims, h_ims, _) = get(&url, &[("if-modified-since", &lm1)]).await;
    println!("dynamic handler: unchanged card, If-Modified-Since={lm1} -> {s_ims}; new Last-Modified={:?}", h_ims.get("last-modified"));
    let (s_inm, _, _) = get(&url, &[("if-none-match", &etag1)]).await;
    println!("dynamic handler: unchanged card, If-None-Match -> {s_inm}");
    assert_eq!(s_inm, 304);

    // 1) File polling
    let mut c2 = c1.clone();
    c2.description = "v2-poll".into();
    tokio::time::sleep(Duration::from_millis(1100)).await; // mtime granularity
    std::fs::write(&path, serde_json::to_vec(&c2).unwrap()).unwrap();
    let mut ok = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let (_, _, b) = get(&url, &[]).await;
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        if v["description"] == "v2-poll" {
            ok = true;
            break;
        }
    }
    println!("poll reload observed: {ok}");
    assert!(ok, "poll watcher did not reload");
    let (s, _, _) = get(&url, &[("if-none-match", &etag1)]).await;
    assert_eq!(s, 200, "old ETag must not match after reload");

    // 2) SIGHUP
    let _sig = hot.spawn_signal_watcher(&path);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut c3 = c1.clone();
    c3.description = "v3-sighup".into();
    // Write the file with the SAME mtime-second risk irrelevant: sighup forces reload.
    std::fs::write(&path, serde_json::to_vec(&c3).unwrap()).unwrap();
    let pid = std::process::id().to_string();
    std::process::Command::new("kill").args(["-HUP", &pid]).status().unwrap();
    let mut ok = false;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if hot.current().description == "v3-sighup" {
            ok = true;
            break;
        }
    }
    println!("sighup reload observed: {ok}");
    assert!(ok, "SIGHUP did not reload");
}

/// The card served by the SDK's own dispatchers cannot be hot reloaded: there is
/// no API to give JsonRpcDispatcher/RestDispatcher/A2aRouter a card producer.
/// This test documents it: after updating a HotReloadAgentCardHandler, the
/// dispatcher still serves the build-time card.
#[tokio::test(flavor = "multi_thread")]
async fn dispatcher_card_is_static() {
    let (agent, _p) = CtlAgent::new();
    let p = port();
    let c1 = card(&format!("http://127.0.0.1:{p}"));
    let hot = HotReloadAgentCardHandler::new(c1.clone());
    let handler = Arc::new(RequestHandlerBuilder::new(agent).with_agent_card(hot.current()).build().unwrap());
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(handler)).await.unwrap();
    let mut c2 = c1.clone();
    c2.description = "v2".into();
    hot.update(c2);
    let (_, _, b) = get(&format!("http://127.0.0.1:{p}/.well-known/agent-card.json"), &[]).await;
    let v: serde_json::Value = serde_json::from_str(&b).unwrap();
    println!("dispatcher-served description after hot.update(): {}", v["description"]);
    assert_eq!(v["description"], "v1");
}

/// Signing: sign with ES256, serve via the SDK, fetch with the SDK client,
/// verify with the SDK; tampering must fail verification.
#[tokio::test(flavor = "multi_thread")]
async fn card_signing_roundtrip_over_wire() {
    let kp = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let pkcs8 = kp.serialize_der();
    let spki = kp.public_key_der();

    let p = port();
    let mut c = card(&format!("http://127.0.0.1:{p}"));
    // Per book/deployment/security.md: declare extensions build() would add before signing.
    c.capabilities.extensions = Some(vec![
        a2a_protocol_sdk::types::extensions::AgentExtension::new(
            a2a_protocol_sdk::types::idempotency::IDEMPOTENCY_EXTENSION_URI,
        ),
        a2a_protocol_sdk::types::extensions::AgentExtension::new(
            a2a_protocol_sdk::types::failure::FAILURE_EXTENSION_URI,
        ),
    ]);
    // Extra: non-ASCII + numbers to exercise RFC 8785 canonicalization on the wire.
    c.description = "Grüße 🚀 \"quoted\" 1e3".into();
    let sig = sign_agent_card(&c, &pkcs8, Some("k1")).unwrap();
    c.signatures = Some(vec![sig]);

    // Negative control: signing before declaring extensions -> build() refuses.
    {
        let mut bad = card("http://127.0.0.1:1");
        let s = sign_agent_card(&bad, &pkcs8, None).unwrap();
        bad.signatures = Some(vec![s]);
        let (a, _) = CtlAgent::new();
        let r = RequestHandlerBuilder::new(a).with_agent_card(bad).build();
        println!("build() with card signed before extensions declared -> is_err={}", r.is_err());
        assert!(r.is_err());
    }

    let (agent, _pr) = CtlAgent::new();
    let handler = Arc::new(RequestHandlerBuilder::new(agent).with_agent_card(c).build().unwrap());
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(handler)).await.unwrap();

    let fetched = resolve_agent_card(&format!("http://127.0.0.1:{p}")).await.unwrap();
    let sigs = fetched.signatures.clone().expect("served card has signatures");
    // Rustdoc of verify_agent_card: `public_key_der` — "DER-encoded public key
    // (SubjectPublicKeyInfo)". Follow the doc first:
    let r_doc = verify_agent_card(&fetched, &sigs[0], &spki);
    println!("verify fetched card with DER SPKI (as documented): {r_doc:?}");
    // What ring's ECDSA_P256_SHA256_FIXED actually takes: the raw SEC1 point (last 65 bytes of SPKI).
    let raw_point = &spki[spki.len() - 65..];
    let r = verify_agent_card(&fetched, &sigs[0], raw_point);
    println!("verify fetched card with raw SEC1 point (undocumented): {r:?}");
    assert!(r.is_ok());
    assert!(r_doc.is_err(), "if this starts passing, the doc/impl mismatch was fixed");
    let spki = raw_point.to_vec();

    let mut tampered = fetched.clone();
    tampered.description.push('!');
    let r2 = verify_agent_card(&tampered, &sigs[0], &spki);
    println!("verify tampered card: is_err={}", r2.is_err());
    assert!(r2.is_err());

    // Wrong key
    let other = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let od = other.public_key_der();
    let r3 = verify_agent_card(&fetched, &sigs[0], &od[od.len() - 65..]);
    println!("verify with wrong key: is_err={}", r3.is_err());
    assert!(r3.is_err());
}
