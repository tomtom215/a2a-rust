// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claim: OpenTelemetry (otel feature) — OtelMetrics exports OTLP over gRPC.
//! A stand-in OTLP collector (hyper HTTP/2) captures the Export request body
//! and we look for the a2a metric names in the protobuf payload.
//! Ports 7950-7999.

use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::otel::{init_otlp_pipeline_with_endpoint, OtelMetricsBuilder};
use bytes::Bytes;
use claims_suite::common::*;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::Frame;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn otlp_export_reaches_collector() {
    let cp = port_in(7950, 50);
    let captured: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Default::default();
    let l = tokio::net::TcpListener::bind(("127.0.0.1", cp)).await.unwrap();
    let cap = captured.clone();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            let cap = cap.clone();
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let cap = cap.clone();
                    async move {
                        let path = req.uri().path().to_owned();
                        let body = req.into_body().collect().await.map(|b| b.to_bytes().to_vec()).unwrap_or_default();
                        cap.lock().unwrap().push((path, body));
                        let mut trailers = hyper::HeaderMap::new();
                        trailers.insert("grpc-status", "0".parse().unwrap());
                        let frames = futures::stream::iter(vec![
                            Ok::<_, Infallible>(Frame::data(Bytes::from_static(&[0, 0, 0, 0, 0]))),
                            Ok(Frame::trailers(trailers)),
                        ]);
                        let mut r = hyper::Response::new(StreamBody::new(frames));
                        r.headers_mut().insert("content-type", "application/grpc".parse().unwrap());
                        Ok::<_, Infallible>(r)
                    }
                });
                let _ = hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(hyper_util::rt::TokioIo::new(s), svc)
                    .await;
            });
        }
    });

    let provider = init_otlp_pipeline_with_endpoint("claims-otel", &format!("http://127.0.0.1:{cp}")).expect("pipeline");
    let metrics = OtelMetricsBuilder::new().build();
    let (agent, _) = CtlAgent::new();
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_metrics(metrics).build().unwrap());
    let p = port_in(7950, 50);
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();
    for _ in 0..3 {
        c.send_message(msg(&uid(), "otel")).await.unwrap();
    }
    let _ = c.get_task(TaskQueryParams::new("missing")).await;
    let pr = provider.clone();
    let fl = tokio::task::spawn_blocking(move || pr.force_flush());
    let r = tokio::time::timeout(Duration::from_secs(15), fl).await;
    println!("force_flush -> {:?}", r.map(|x| x.map(|y| y.map_err(|e| e.to_string()))));
    let cap = captured.lock().unwrap().clone();
    let total: usize = cap.iter().map(|(_, b)| b.len()).sum();
    let all: Vec<u8> = cap.iter().flat_map(|(_, b)| b.clone()).collect();
    let text = String::from_utf8_lossy(&all);
    let names = ["a2a.server.requests", "a2a.server.responses", "a2a.server.errors", "a2a.server.latency", "a2a.server.queue_depth", "claims-otel"];
    let found: Vec<_> = names.iter().map(|n| (n, text.contains(n))).collect();
    println!("collector got {} request(s) to {:?}, {} bytes; metric names present: {found:?}", cap.len(), cap.iter().map(|x| x.0.clone()).collect::<Vec<_>>(), total);
    assert!(cap.iter().any(|(p, _)| p.contains("MetricsService/Export")));
    assert!(text.contains("a2a.server.requests"));
    assert!(text.contains("a2a.server.latency"));
    let _ = tokio::task::spawn_blocking(move || provider.shutdown()).await;
}
