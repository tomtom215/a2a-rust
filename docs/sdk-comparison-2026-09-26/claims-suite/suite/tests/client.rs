// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: client RetryPolicy, idempotency keys (+ card advertisement),
//! TLS client (rustls), A2aRouter axum integration. Ports 7750-7799.

use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{A2aRouter, GrpcConfig, GrpcDispatcher, SqliteTaskStore};
use a2a_protocol_sdk::types::idempotency::{set_key, IDEMPOTENCY_EXTENSION_URI};
use bytes::Bytes;
use claims_suite::common::*;
use http_body_util::{BodyExt, Full};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn port() -> u16 {
    port_in(7750, 50)
}

// ── Scripted fake server ─────────────────────────────────────────────────────
async fn fake_server(script: Vec<u16>) -> (u16, Arc<AtomicUsize>) {
    let p = port();
    let hits = Arc::new(AtomicUsize::new(0));
    let script = Arc::new(script);
    let l = tokio::net::TcpListener::bind(("127.0.0.1", p)).await.unwrap();
    let h2 = hits.clone();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            let hits = h2.clone();
            let script = script.clone();
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let hits = hits.clone();
                    let script = script.clone();
                    async move {
                        let n = hits.fetch_add(1, Ordering::SeqCst);
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
                        let status = *script.get(n).unwrap_or(&200);
                        let out = if status == 200 {
                            serde_json::json!({"jsonrpc":"2.0","id":v["id"],"result":{"task":{
                                "id":"t1","contextId":"c1","status":{"state":"TASK_STATE_COMPLETED"}}}})
                            .to_string()
                        } else {
                            "{}".to_owned()
                        };
                        let mut r = hyper::Response::new(Full::new(Bytes::from(out)));
                        *r.status_mut() = hyper::StatusCode::from_u16(status).unwrap();
                        r.headers_mut().insert("content-type", "application/json".parse().unwrap());
                        Ok::<_, Infallible>(r)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(s), svc)
                    .await;
            });
        }
    });
    (p, hits)
}

#[tokio::test(flavor = "multi_thread")]
async fn retry_policy_matrix() {
    let pol = || RetryPolicy::default().with_initial_backoff(Duration::from_millis(20)).with_max_retries(3);
    let mut rows = vec![];
    for (label, script, op) in [
        ("send 503,503,200", vec![503, 503, 200], "send"),
        ("send 400", vec![400, 200], "send"),
        ("send 429,200", vec![429, 200], "send"),
        ("send 502,200", vec![502, 200], "send"),
        ("send 504,200", vec![504, 200], "send"),
        ("get 502,200", vec![502, 200], "get"),
        ("get 503,200", vec![503, 200], "get"),
        ("get 504,200", vec![504, 200], "get"),
        ("get 500,200", vec![500, 200], "get"),
        ("get 503x5", vec![503, 503, 503, 503, 503, 503], "get"),
    ] {
        let (p, hits) = fake_server(script).await;
        let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).with_retry_policy(pol()).build().unwrap();
        let ok = if op == "send" {
            c.send_message(msg(&uid(), "x")).await.map(|_| ()).map_err(|e| format!("{e}"))
        } else {
            c.get_task(TaskQueryParams::new("t1")).await.map(|_| ()).map_err(|e| format!("{e}"))
        };
        let row = format!("{label}: hits={} result={:?}", hits.load(Ordering::SeqCst), ok);
        println!("{row}");
        rows.push((label, hits.load(Ordering::SeqCst), ok.is_ok()));
    }
    let get = |l: &str| rows.iter().find(|r| r.0 == l).unwrap().clone();
    assert_eq!(get("send 503,503,200").1, 3);
    assert!(get("send 503,503,200").2);
    assert_eq!(get("send 400").1, 1);
    assert!(!get("send 400").2);
    assert_eq!(get("get 502,200").1, 2);
    assert_eq!(get("get 504,200").1, 2);
    assert_eq!(get("get 500,200").1, 1);
    assert_eq!(get("get 503x5").1, 4, "1 + max_retries(3)");
}

// ── Idempotency ──────────────────────────────────────────────────────────────
fn keyed(key: &str, text: &str, mid: &str) -> MessageSendParams {
    let mut m = Message::user_text(mid, text);
    set_key(&mut m, key).unwrap();
    MessageSendParams::new(m)
}

#[tokio::test(flavor = "multi_thread")]
async fn idempotency_server_dedupe_and_card() {
    for store in ["in-memory", "sqlite"] {
        let (agent, probe) = CtlAgent::new();
        let p = port();
        let card = AgentCard::new("idem", "1.0.0", AgentInterface::jsonrpc(format!("http://127.0.0.1:{p}")));
        let mut b = RequestHandlerBuilder::new(agent).with_agent_card(card);
        if store == "sqlite" {
            b = b.with_task_store(SqliteTaskStore::new("sqlite::memory:").await.unwrap());
        }
        let h = Arc::new(b.build().unwrap());
        serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
        let card = resolve_agent_card(&format!("http://127.0.0.1:{p}")).await.unwrap();
        let adv = card.capabilities.extensions.unwrap_or_default().iter().any(|e| e.uri == IDEMPOTENCY_EXTENSION_URI);
        let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();
        let key = "k-0123456789abcdef0123456789";
        let t1 = task_of(c.send_message(keyed(key, "one", "mid-1")).await.unwrap());
        let t2 = task_of(c.send_message(keyed(key, "one", "mid-1")).await.unwrap());
        let t3 = c.send_message(keyed(key, "one", "mid-2")).await;
        let t3d = match &t3 {
            Ok(SendMessageResponse::Task(t)) => format!("task {}", t.id),
            other => format!("{other:?}"),
        };
        let other = task_of(c.send_message(keyed("k-ffffffffffffffffffffffff", "two", "mid-3")).await.unwrap());
        println!(
            "{store}: advertised={adv} t1={} t2={} same={} new-msgid-same-key -> {t3d}; different key -> {} ; executions={}",
            t1.id, t2.id, t1.id == t2.id, other.id, probe.executions.load(Ordering::SeqCst)
        );
        assert!(adv, "{store}: extension advertised on card");
        assert_eq!(t1.id, t2.id, "{store}: dedupe");
        assert_ne!(t1.id, other.id);
        assert_eq!(probe.executions.load(Ordering::SeqCst), 2, "{store}: agent ran once per key");
    }
}

/// Ambiguous failure: a proxy forwards the first SendMessage to the server
/// then drops the client connection without a response. With a key, a peer
/// that honours it (from_card), and a RetryPolicy, the client retries and gets
/// the original task; the agent runs once. Without a key: no retry.
#[tokio::test(flavor = "multi_thread")]
async fn idempotency_client_retry_after_ambiguous_failure() {
    let (agent, probe) = CtlAgent::new();
    let backend = port();
    let front = port();
    let card = AgentCard::new("idem", "1.0.0", AgentInterface::jsonrpc(format!("http://127.0.0.1:{front}")));
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_agent_card(card).build().unwrap());
    serve_with_addr(format!("127.0.0.1:{backend}"), JsonRpcDispatcher::new(h)).await.unwrap();

    // Proxy: POSTs are forwarded; the response to the Nth POST is dropped when drop_next is set.
    let drop_next = Arc::new(Mutex::new(false));
    let posts = Arc::new(AtomicUsize::new(0));
    let l = tokio::net::TcpListener::bind(("127.0.0.1", front)).await.unwrap();
    let (dn, ps) = (drop_next.clone(), posts.clone());
    tokio::spawn(async move {
        loop {
            let (mut cs, _) = l.accept().await.unwrap();
            let (dn, ps) = (dn.clone(), ps.clone());
            tokio::spawn(async move {
                async fn read_msg(s: &mut tokio::net::TcpStream) -> Option<Vec<u8>> {
                    let mut buf = vec![];
                    let mut tmp = [0u8; 8192];
                    loop {
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                            let cl: usize = head.lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                            if buf.len() >= pos + 4 + cl { return Some(buf); }
                        }
                        let n = s.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 { return None; }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                }
                // keep-alive loop: one backend connection per request, full request/response framing
                loop {
                    let Some(req) = read_msg(&mut cs).await else { return };
                    let is_post = req.starts_with(b"POST");
                    if is_post { ps.fetch_add(1, Ordering::SeqCst); }
                    let mut bs = tokio::net::TcpStream::connect(("127.0.0.1", backend)).await.unwrap();
                    bs.write_all(&req).await.unwrap();
                    let drop_it = is_post && { let mut g = dn.lock().unwrap(); let d = *g; *g = false; d };
                    if drop_it {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        drop(cs);
                        return;
                    }
                    let Some(resp) = read_msg(&mut bs).await else { return };
                    if cs.write_all(&resp).await.is_err() { return; }
                }
            });
        }
    });

    let card = resolve_agent_card(&format!("http://127.0.0.1:{front}")).await.unwrap();
    let client = ClientBuilder::from_card(&card)
        .unwrap()
        .with_retry_policy(RetryPolicy::default().with_initial_backoff(Duration::from_millis(50)))
        .build()
        .unwrap();

    // Keyed
    *drop_next.lock().unwrap() = true;
    let before = probe.executions.load(Ordering::SeqCst);
    let r = client.send_message(keyed("k-ambiguous-0123456789abcdef", "keyed", "mid-a")).await;
    let execs = probe.executions.load(Ordering::SeqCst) - before;
    println!("keyed send after dropped response: ok={} posts={} executions={execs} err={:?}",
        r.is_ok(), posts.load(Ordering::SeqCst), r.as_ref().err());

    // Unkeyed
    *drop_next.lock().unwrap() = true;
    let posts_before = posts.load(Ordering::SeqCst);
    let before = probe.executions.load(Ordering::SeqCst);
    let r2 = client.send_message(msg("mid-b", "unkeyed")).await;
    let execs2 = probe.executions.load(Ordering::SeqCst) - before;
    println!("unkeyed send after dropped response: ok={} posts={} executions={execs2} err={:?}",
        r2.is_ok(), posts.load(Ordering::SeqCst) - posts_before, r2.as_ref().err());

    assert!(r.is_ok(), "keyed send should be retried and succeed");
    assert_eq!(execs, 1, "agent ran once");
    assert!(r2.is_err(), "unkeyed ambiguous send must not be retried");
    assert_eq!(posts.load(Ordering::SeqCst) - posts_before, 1);
}

// ── TLS ──────────────────────────────────────────────────────────────────────
fn self_signed() -> (rcgen::CertifiedKey,) {
    (rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()]).unwrap(),)
}

#[tokio::test(flavor = "multi_thread")]
async fn tls_https_jsonrpc_selfsigned() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (ck,) = self_signed();
    let cert_der = ck.cert.der().clone();
    let key_der = rustls::pki_types::PrivateKeyDer::try_from(ck.key_pair.serialize_der()).unwrap();
    let cfg = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(vec![cert_der.clone()], key_der).unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));

    let (agent, _probe) = CtlAgent::new();
    let h = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());
    let disp = Arc::new(JsonRpcDispatcher::new(h));
    let p = port();
    let l = tokio::net::TcpListener::bind(("127.0.0.1", p)).await.unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            let acc = acceptor.clone();
            let d = disp.clone();
            tokio::spawn(async move {
                let Ok(tls) = acc.accept(s).await else { return };
                let svc = hyper::service::service_fn(move |req| {
                    let d = d.clone();
                    async move { Ok::<_, Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(hyper_util::rt::TokioIo::new(tls), svc)
                    .await;
            });
        }
    });
    // Sanity: a reqwest client trusting the cert talks to it.
    let rc = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_der(&cert_der).unwrap())
        .build()
        .unwrap();
    let st = rc.get(format!("https://localhost:{p}/.well-known/agent-card.json")).send().await.unwrap().status();
    println!("reqwest(trusting self-signed) over https -> {st}");

    let c = ClientBuilder::new(format!("https://localhost:{p}")).build().unwrap();
    let r = c.send_message(msg(&uid(), "tls")).await;
    println!("SDK client (default roots) -> https self-signed: {:?}", r.as_ref().map(|_| "OK").map_err(|e| e.to_string()));
    assert!(r.is_err(), "must refuse an untrusted cert");
    // The SDK exposes tls_config_with_extra_roots(), but no ClientBuilder / JsonRpcTransport /
    // RestTransport API accepts a rustls ClientConfig (source-verified), so there is no way
    // to make the HTTP client trust this CA.
    let _unused = a2a_protocol_sdk::client::tls::tls_config_with_extra_roots(vec![cert_der]);
}

#[tokio::test(flavor = "multi_thread")]
async fn tls_grpc_private_ca() {
    use a2a_protocol_sdk::client::transport::grpc::ClientTlsConfig;
    use a2a_protocol_sdk::server::dispatch::grpc::{Certificate, Identity, ServerTlsConfig};
    let (ck,) = self_signed();
    let cert_pem = ck.cert.pem();
    let key_pem = ck.key_pair.serialize_pem();
    let (agent, _probe) = CtlAgent::new();
    let h = Arc::new(RequestHandlerBuilder::new(agent).build().unwrap());
    let p = port();
    GrpcDispatcher::new(h, GrpcConfig::default())
        .with_tls(ServerTlsConfig::new().identity(Identity::from_pem(&cert_pem, &key_pem)))
        .serve_with_addr(format!("127.0.0.1:{p}"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let c = ClientBuilder::new(format!("https://localhost:{p}"))
        .with_grpc_tls_config(
            ClientTlsConfig::new()
                .ca_certificate(a2a_protocol_sdk::client::transport::grpc::Certificate::from_pem(&cert_pem))
                .domain_name("localhost"),
        )
        .build_grpc()
        .await
        .expect("grpc tls build");
    let t = task_of(c.send_message(msg(&uid(), "tls")).await.expect("grpc over tls"));
    println!("gRPC over TLS (private CA via with_grpc_tls_config): {:?}", t.text());
    assert_eq!(t.text(), Some("Hello, tls!"));
    let _ = Certificate::from_pem(&cert_pem);
}

// ── Axum ─────────────────────────────────────────────────────────────────────
#[tokio::test(flavor = "multi_thread")]
async fn axum_a2a_router_integration() {
    let (agent, _probe) = CtlAgent::new();
    let p = port();
    let card = AgentCard::new("ax", "1.0.0", AgentInterface::rest(format!("http://127.0.0.1:{p}")))
        .with_capabilities(AgentCapabilities::none().with_streaming(true));
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_agent_card(card).build().unwrap());
    let app = axum::Router::new()
        .merge(A2aRouter::new(h).into_router())
        .route("/custom", axum::routing::get(|| async { "custom-ok" }));
    let l = tokio::net::TcpListener::bind(("127.0.0.1", p)).await.unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).with_protocol_binding("REST").build().unwrap();
    let t = task_of(c.send_message(msg(&uid(), "axum")).await.unwrap());
    assert_eq!(t.text(), Some("Hello, axum!"));
    let mut s = c.stream_message(msg(&uid(), "chunks:3:50")).await.unwrap();
    let mut n = 0;
    while let Some(ev) = s.next().await {
        if let StreamResponse::ArtifactUpdate(_) = ev.unwrap() { n += 1; }
    }
    let g = c.get_task(TaskQueryParams::new(t.id.to_string())).await.unwrap();
    let cancel = c.stream_message(msg(&uid(), "sleep:5000")).await;
    let custom = reqwest::get(format!("http://127.0.0.1:{p}/custom")).await.unwrap().text().await.unwrap();
    let health = reqwest::get(format!("http://127.0.0.1:{p}/health")).await.unwrap().status();
    let ready = reqwest::get(format!("http://127.0.0.1:{p}/ready")).await.unwrap().text().await.unwrap();
    println!("axum: send ok, streamed artifacts={n}, get={:?}, custom={custom}, /health={health}, /ready={ready}, stream-open={}", g.status.state, cancel.is_ok());
    assert_eq!(n, 3);
    assert_eq!(custom, "custom-ok");
}
