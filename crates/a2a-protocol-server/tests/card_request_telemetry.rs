// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A request for the agent card is an HTTP `SERVER` span and a point in
//! `http.server.request.duration`, by the HTTP conventions, as exported. Its
//! own test binary, because the meter provider is process-global.

#![cfg(feature = "otel")]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_server::{JsonRpcDispatcher, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
use opentelemetry::trace::{SpanKind, Status, TracerProvider as _};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanExporter};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tracing_subscriber::layer::SubscriberExt as _;

#[derive(Clone, Debug, Default)]
struct Keep(Arc<Mutex<Vec<SpanData>>>);

impl SpanExporter for Keep {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        self.0.lock().unwrap().extend(batch);
        std::future::ready(Ok(()))
    }
}

#[derive(Debug)]
struct SharedReader(Arc<ManualReader>);

impl MetricReader for SharedReader {
    fn register_pipeline(&self, pipeline: std::sync::Weak<opentelemetry_sdk::metrics::Pipeline>) {
        self.0.register_pipeline(pipeline);
    }
    fn collect(&self, rm: &mut ResourceMetrics) -> OTelSdkResult {
        self.0.collect(rm)
    }
    fn force_flush(&self) -> OTelSdkResult {
        self.0.force_flush()
    }
    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.0.shutdown_with_timeout(timeout)
    }
    fn temporality(
        &self,
        kind: opentelemetry_sdk::metrics::InstrumentKind,
    ) -> opentelemetry_sdk::metrics::Temporality {
        self.0.temporality(kind)
    }
}

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

/// Sends `head` (a request line and headers) to `addr` and returns the
/// response's status.
async fn send(addr: std::net::SocketAddr, head: &str) -> u16 {
    let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(format!("{head}connection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    response[9..12].parse().unwrap()
}

fn attributes(span: &SpanData) -> BTreeMap<String, String> {
    span.attributes
        .iter()
        .map(|kv| (kv.key.to_string(), kv.value.to_string()))
        .collect()
}

#[test]
fn a_card_request_is_an_http_server_span_and_a_duration() {
    let reader = Arc::new(ManualReader::builder().build());
    let meters = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    opentelemetry::global::set_meter_provider(meters.clone());
    let keep = Keep::default();
    let tracer = SdkTracerProvider::builder()
        .with_simple_exporter(keep.clone())
        .build();
    let layer = tracing_opentelemetry::layer().with_tracer(tracer.tracer("card"));

    let statuses =
        tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let card =
                    AgentCard::new("a", "1.0.0", AgentInterface::jsonrpc("http://localhost:1"))
                        .with_description("d")
                        .with_skill(AgentSkill::new("s", "S", "d").with_tags(["t"]));
                let with_card = RequestHandlerBuilder::new(Idle)
                    .with_agent_card(card)
                    .build()
                    .unwrap();
                let without = RequestHandlerBuilder::new(Idle)
                    .without_agent_card()
                    .build()
                    .unwrap();
                let served = a2a_protocol_server::serve_with_addr(
                    "127.0.0.1:0",
                    JsonRpcDispatcher::new(Arc::new(with_card)),
                )
                .await
                .unwrap();
                let bare = a2a_protocol_server::serve_with_addr(
                    "127.0.0.1:0",
                    JsonRpcDispatcher::new(Arc::new(without)),
                )
                .await
                .unwrap();
                vec![
                    send(
                        served,
                        &format!(
                            "GET /.well-known/agent-card.json?key=SECRET HTTP/1.1\r\n\
                         host: agent.example.com:8443\r\nuser-agent: probe/1\r\n\
                         traceparent: 00-{TRACE_ID}-00f067aa0ba902b7-01\r\n"
                        ),
                    )
                    .await,
                    send(
                        served,
                        "POST /.well-known/agent-card.json HTTP/1.1\r\nhost: h\r\n",
                    )
                    .await,
                    send(
                        served,
                        "FROB /.well-known/agent-card.json HTTP/1.1\r\nhost: h\r\n",
                    )
                    .await,
                    send(
                        bare,
                        "GET /.well-known/agent-card.json HTTP/1.1\r\nhost: h\r\n",
                    )
                    .await,
                ]
            })
        });
    assert_eq!(statuses, [200, 405, 405, 404]);

    let spans = keep.0.lock().unwrap().clone();
    let cards: Vec<&SpanData> = spans
        .iter()
        .filter(|s| s.name.ends_with("/.well-known/agent-card.json"))
        .collect();
    assert_eq!(
        cards.len(),
        4,
        "{:?}",
        spans.iter().map(|s| &s.name).collect::<Vec<_>>()
    );

    let get = cards[0];
    assert_eq!(get.name, "GET /.well-known/agent-card.json");
    assert_eq!(get.span_kind, SpanKind::Server);
    assert_eq!(get.status, Status::Unset);
    assert_eq!(
        get.span_context.trace_id().to_string(),
        TRACE_ID,
        "the caller's traceparent is the parent (InboundTracePolicy::Continue)"
    );
    let a = attributes(get);
    for (key, value) in [
        ("http.request.method", "GET"),
        ("http.route", "/.well-known/agent-card.json"),
        ("url.path", "/.well-known/agent-card.json"),
        ("url.scheme", "http"),
        ("url.query", "REDACTED"),
        ("http.response.status_code", "200"),
        ("network.protocol.version", "1.1"),
        ("network.peer.address", "127.0.0.1"),
        ("server.address", "agent.example.com"),
        ("server.port", "8443"),
        ("user_agent.original", "probe/1"),
    ] {
        assert_eq!(a.get(key).map(String::as_str), Some(value), "{key}: {a:?}");
    }
    assert!(!a.contains_key("error.type"), "{a:?}");

    // A 4xx leaves a server span's status unset and sets no error.type.
    let post = attributes(cards[1]);
    assert_eq!(
        post.get("http.response.status_code").map(String::as_str),
        Some("405")
    );
    assert!(!post.contains_key("error.type"));
    assert_eq!(cards[1].status, Status::Unset);

    // A method the conventions do not know is `_OTHER`, and the span `HTTP`.
    assert_eq!(cards[2].name, "HTTP /.well-known/agent-card.json");
    let other = attributes(cards[2]);
    assert_eq!(
        other.get("http.request.method").map(String::as_str),
        Some("_OTHER")
    );
    assert_eq!(
        other
            .get("http.request.method_original")
            .map(String::as_str),
        Some("FROB")
    );

    assert_eq!(
        attributes(cards[3])
            .get("http.response.status_code")
            .map(String::as_str),
        Some("404")
    );

    // One point per method and status, with the conventions' attributes.
    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).unwrap();
    let mut points: Vec<(BTreeMap<String, String>, u64)> = Vec::new();
    for scope in collected.scope_metrics() {
        for metric in scope.metrics() {
            if metric.name() != "http.server.request.duration" {
                continue;
            }
            assert_eq!(metric.unit(), "s");
            let AggregatedMetrics::F64(MetricData::Histogram(h)) = metric.data() else {
                panic!("not an f64 histogram");
            };
            for p in h.data_points() {
                points.push((
                    p.attributes()
                        .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                        .collect(),
                    p.count(),
                ));
            }
        }
    }
    let point = |method: &str, status: &str| {
        points
            .iter()
            .find(|(a, _)| {
                a.get("http.request.method").map(String::as_str) == Some(method)
                    && a.get("http.response.status_code").map(String::as_str) == Some(status)
            })
            .unwrap_or_else(|| panic!("no {method} {status} point among {points:?}"))
    };
    let (get_point, get_count) = point("GET", "200");
    assert_eq!(*get_count, 1);
    assert_eq!(
        get_point.get("http.route").map(String::as_str),
        Some("/.well-known/agent-card.json")
    );
    assert_eq!(
        get_point.get("url.scheme").map(String::as_str),
        Some("http")
    );
    assert!(
        !get_point.contains_key("server.address"),
        "no caller-chosen attribute"
    );
    assert_eq!(point("POST", "405").1, 1);
    assert_eq!(point("_OTHER", "405").1, 1);
    assert_eq!(point("GET", "404").1, 1);

    every_listener_records_its_card_requests();
}

/// Every listener that answers the card records its request, not only the
/// JSON-RPC one: HTTP+JSON, the axum router, WebSocket and gRPC each export
/// one `GET` span with the response's status. Called from the test above
/// once its metric points are counted, since the meter provider is shared.
fn every_listener_records_its_card_requests() {
    let keep = Keep::default();
    let tracer = SdkTracerProvider::builder()
        .with_simple_exporter(keep.clone())
        .build();
    let layer = tracing_opentelemetry::layer().with_tracer(tracer.tracer("card"));
    let card = || {
        AgentCard::new("a", "1.0.0", AgentInterface::jsonrpc("http://localhost:1"))
            .with_description("d")
            .with_skill(AgentSkill::new("s", "S", "d").with_tags(["t"]))
    };
    let handler = || {
        Arc::new(
            RequestHandlerBuilder::new(Idle)
                .with_agent_card(card())
                .build()
                .unwrap(),
        )
    };
    let get = "GET /.well-known/agent-card.json HTTP/1.1\r\nhost: h\r\n";
    let mut expected: Vec<&str> = Vec::new();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let rest = a2a_protocol_server::serve_with_addr(
                "127.0.0.1:0",
                a2a_protocol_server::RestDispatcher::new(handler()),
            )
            .await
            .unwrap();
            assert_eq!(send(rest, get).await, 200);
            expected.push("rest");
            #[cfg(feature = "axum")]
            {
                let app = a2a_protocol_server::dispatch::axum_adapter::A2aRouter::new(handler())
                    .into_router();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();
                tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
                assert_eq!(send(addr, get).await, 200);
                expected.push("axum");
            }
            #[cfg(feature = "websocket")]
            {
                let addr = Arc::new(a2a_protocol_server::dispatch::WebSocketDispatcher::new(
                    handler(),
                ))
                .serve_with_addr("127.0.0.1:0")
                .await
                .unwrap();
                assert_eq!(send(addr, get).await, 200);
                expected.push("websocket");
            }
            #[cfg(feature = "grpc")]
            {
                let addr = a2a_protocol_server::GrpcDispatcher::new(
                    handler(),
                    a2a_protocol_server::GrpcConfig::default(),
                )
                .serve_with_addr("127.0.0.1:0")
                .await
                .unwrap();
                assert_eq!(send(addr, get).await, 200);
                expected.push("grpc");
            }
        });
    });
    let spans = keep.0.lock().unwrap().clone();
    let cards: Vec<BTreeMap<String, String>> = spans
        .iter()
        .filter(|s| s.name == "GET /.well-known/agent-card.json")
        .map(attributes)
        .collect();
    assert_eq!(
        cards.len(),
        expected.len(),
        "listeners {expected:?}, spans {cards:?}"
    );
    for a in &cards {
        assert_eq!(
            a.get("http.response.status_code").map(String::as_str),
            Some("200")
        );
    }
    // The listeners that know the connection's peer record it: all but
    // axum served without `ConnectInfo`.
    let with_peer = cards
        .iter()
        .filter(|a| a.contains_key("network.peer.address"))
        .count();
    assert!(
        with_peer >= expected.len() - usize::from(expected.contains(&"axum")),
        "{cards:?}"
    );
}
