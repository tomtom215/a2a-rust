// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: Push notifications (HttpPushSender to a local webhook, HTTPS
//! webhook, SSRF protection, header-injection prevention, metrics
//! on_push_delivery). Ports 7850-7899.

use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::HttpPushSender;
use a2a_protocol_sdk::types::push::{AuthenticationInfo, TaskPushNotificationConfig};
use bytes::Bytes;
use claims_suite::common::*;
use http_body_util::{BodyExt, Full};

fn port() -> u16 {
    port_in(7850, 50)
}

type Log = Arc<Mutex<Vec<(Vec<(String, String)>, String)>>>;

fn hook_service(log: Log) -> impl Fn(hyper::Request<hyper::body::Incoming>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<hyper::Response<Full<Bytes>>, Infallible>> + Send>> + Clone {
    move |req: hyper::Request<hyper::body::Incoming>| {
        let log = log.clone();
        Box::pin(async move {
            let hs: Vec<(String, String)> = req
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_owned()))
                .collect();
            let body = String::from_utf8_lossy(&req.into_body().collect().await.unwrap().to_bytes()).to_string();
            log.lock().unwrap().push((hs, body));
            Ok(hyper::Response::new(Full::new(Bytes::from_static(b"ok"))))
        })
    }
}

async fn plain_hook(p: u16, log: Log) {
    let l = tokio::net::TcpListener::bind(("127.0.0.1", p)).await.unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            let svc = hook_service(log.clone());
            tokio::spawn(async move {
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(s), hyper::service::service_fn(svc))
                    .await;
            });
        }
    });
}

fn card(p: u16) -> AgentCard {
    AgentCard::new("push", "1.0.0", AgentInterface::jsonrpc(format!("http://127.0.0.1:{p}")))
        .with_capabilities(AgentCapabilities::none().with_push_notifications(true).with_streaming(true))
}

async fn run_push(sender: HttpPushSender, hook_url: String, metrics: RecMetrics) -> (Result<(), String>, A2aClient, String) {
    let (agent, _) = CtlAgent::new();
    let p = port();
    let h = Arc::new(
        RequestHandlerBuilder::new(agent)
            .with_agent_card(card(p))
            .with_push_sender(sender)
            .with_metrics(metrics)
            .build()
            .unwrap(),
    );
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).with_return_immediately(true).build().unwrap();
    let t = task_of(c.send_message(msg(&uid(), "sleep:1200")).await.unwrap());
    let mut cfg = TaskPushNotificationConfig::new(t.id.to_string(), hook_url);
    cfg.token = Some("tok-123".into());
    cfg.authentication = Some(AuthenticationInfo { scheme: "Bearer".into(), credentials: Some("webhook-secret".into()) });
    let r = c.set_push_config(cfg).await.map(|_| ()).map_err(|e| e.to_string());
    (r, c, t.id.to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn push_to_local_webhook() {
    // 1) Default sender: SSRF protection refuses a loopback webhook.
    let hp = port();
    let log: Log = Default::default();
    plain_hook(hp, log.clone()).await;
    let (r, _, _) = run_push(HttpPushSender::new(), format!("http://127.0.0.1:{hp}/hook"), RecMetrics::default()).await;
    println!("default HttpPushSender, loopback webhook: set_push_config -> {r:?}");
    assert!(r.is_err(), "SSRF: loopback refused by default");

    // 2) allow_private_urls(): deliveries arrive.
    let m = RecMetrics::default();
    let (r, _c, tid) = run_push(HttpPushSender::new().allow_private_urls(), format!("http://127.0.0.1:{hp}/hook"), m.clone()).await;
    println!("allow_private_urls: set_push_config -> {r:?}");
    assert!(r.is_ok());
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let got = log.lock().unwrap().clone();
    println!("webhook received {} POST(s)", got.len());
    for (hs, b) in &got {
        let pick = |k: &str| hs.iter().find(|(n, _)| n == k).map(|x| x.1.clone());
        println!("  content-type={:?} x-a2a-notification-token={:?} a2a-notification-token={:?} authorization={:?} body={}",
            pick("content-type"), pick("x-a2a-notification-token"), pick("a2a-notification-token"), pick("authorization"), &b[..b.len().min(140)]);
    }
    println!("metrics on_push_delivery outcomes: {:?}", m.0.push.lock().unwrap());
    assert!(!got.is_empty());
    assert!(got.iter().any(|(_, b)| b.contains("TASK_STATE_COMPLETED") && b.contains(&tid)));
    let (hs, _) = &got[0];
    assert!(hs.iter().any(|(k, v)| k == "x-a2a-notification-token" && v == "tok-123"));
    assert!(hs.iter().any(|(k, v)| k == "a2a-notification-token" && v == "tok-123"));
    assert!(hs.iter().any(|(k, v)| k == "authorization" && v == "Bearer webhook-secret"));
    assert!(m.count("on_push_delivery") > 0, "Metrics::on_push_delivery fired");

    // 3) Header injection in credentials is refused.
    let (agent, _) = CtlAgent::new();
    let p = port();
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_agent_card(card(p)).with_push_sender(HttpPushSender::new().allow_private_urls()).build().unwrap());
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).with_return_immediately(true).build().unwrap();
    let t = task_of(c.send_message(msg(&uid(), "sleep:500")).await.unwrap());
    let mut cfg = TaskPushNotificationConfig::new(t.id.to_string(), format!("http://127.0.0.1:{hp}/hook"));
    cfg.authentication = Some(AuthenticationInfo { scheme: "Bearer".into(), credentials: Some("x\r\nX-Evil: 1".into()) });
    let r = c.set_push_config(cfg).await;
    println!("CRLF in credentials: set_push_config -> {:?}", r.as_ref().map(|_| ()).map_err(|e| e.to_string()));
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let evil = log.lock().unwrap().iter().any(|(hs, _)| hs.iter().any(|(k, _)| k == "x-evil"));
    println!("X-Evil header observed at webhook: {evil}");
    assert!(!evil);
}

#[tokio::test(flavor = "multi_thread")]
async fn push_to_https_webhook() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_der = ck.cert.der().clone();
    let key = rustls::pki_types::PrivateKeyDer::try_from(ck.key_pair.serialize_der()).unwrap();
    let scfg = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(vec![cert_der.clone()], key).unwrap();
    let acc = tokio_rustls::TlsAcceptor::from(Arc::new(scfg));
    let hp = port();
    let log: Log = Default::default();
    let l = tokio::net::TcpListener::bind(("127.0.0.1", hp)).await.unwrap();
    let lg = log.clone();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            let acc = acc.clone();
            let svc = hook_service(lg.clone());
            tokio::spawn(async move {
                let Ok(tls) = acc.accept(s).await else { return };
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(tls), hyper::service::service_fn(svc))
                    .await;
            });
        }
    });
    let tls = a2a_protocol_sdk::client::tls::tls_config_with_extra_roots(vec![cert_der]);
    let sender = HttpPushSender::with_tls_config(tls).allow_private_urls();
    let (r, _c, _tid) = run_push(sender, format!("https://localhost:{hp}/hook"), RecMetrics::default()).await;
    println!("https webhook: set_push_config -> {r:?}");
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let n = log.lock().unwrap().len();
    println!("https webhook received {n} POST(s)");
    assert!(r.is_ok());
    assert!(n > 0);
}
