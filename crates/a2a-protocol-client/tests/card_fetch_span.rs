// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An agent-card fetch's span as exported: the HTTP conventions' `CLIENT`
//! span for its `GET`, with the response's status and, on failure, why.

#![cfg(feature = "otel")]

use std::sync::{Arc, Mutex};

use a2a_protocol_client::discovery::fetch_card_from_url;
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
use opentelemetry::trace::{SpanKind, Status, TracerProvider as _};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanExporter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

/// Serves one response, `status` with `body`, and returns the URL to fetch
/// (with a query, which the span must not record).
async fn serve(status: &'static str, body: String) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/.well-known/agent-card.json?key=SECRET",
        listener.local_addr().unwrap()
    );
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let mut buf = [0_u8; 8192];
        let _ = stream.read(&mut buf).await;
        let resp = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
    });
    url
}

fn card(skills: bool) -> String {
    let mut card = AgentCard::new("a", "1.0.0", AgentInterface::jsonrpc("http://localhost:1"))
        .with_description("d");
    if skills {
        card = card.with_skill(AgentSkill::new("s", "S", "d").with_tags(["t"]));
    }
    serde_json::to_string(&card).unwrap()
}

/// Fetches from a server answering `status` with `body`, and returns the
/// fetch's exported span and the URL fetched.
fn fetched(status: &'static str, body: String) -> (SpanData, String) {
    let keep = Keep::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(keep.clone())
        .build();
    let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("card-test"));
    let url = tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let url = serve(status, body).await;
            let _ = fetch_card_from_url(&url).await;
            url
        })
    });
    let spans = std::mem::take(&mut *keep.0.lock().unwrap());
    let span = spans
        .into_iter()
        .find(|s| s.name == "GET")
        .expect("a GET span");
    (span, url)
}

fn attr(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

#[test]
fn a_fetched_card_is_a_get_client_span_with_its_status() {
    let (span, url) = fetched("200 OK", card(true));
    assert_eq!(span.span_kind, SpanKind::Client);
    assert_eq!(attr(&span, "http.request.method").as_deref(), Some("GET"));
    assert_eq!(
        attr(&span, "http.response.status_code").as_deref(),
        Some("200")
    );
    assert_eq!(attr(&span, "server.address").as_deref(), Some("127.0.0.1"));
    let port = url.split(':').nth(2).unwrap().split('/').next().unwrap();
    assert_eq!(attr(&span, "server.port").as_deref(), Some(port));
    let full = attr(&span, "url.full").unwrap();
    assert!(full.ends_with("/.well-known/agent-card.json"), "{full}");
    assert!(!full.contains("SECRET"), "the query is dropped: {full}");
    assert_eq!(attr(&span, "error.type"), None);
    assert!(!matches!(span.status, Status::Error { .. }));
}

#[test]
fn a_refused_fetch_records_the_status_as_its_error() {
    let (span, _) = fetched("404 Not Found", String::new());
    assert_eq!(
        attr(&span, "http.response.status_code").as_deref(),
        Some("404")
    );
    assert_eq!(attr(&span, "error.type").as_deref(), Some("404"));
    assert!(matches!(span.status, Status::Error { .. }));
}

/// A card the client refuses for a2a.proto's field rules (no skill) came
/// back `200`, and failed as `invalid_response`.
#[test]
fn a_card_that_breaks_the_field_rules_is_an_invalid_response() {
    let (span, _) = fetched("200 OK", card(false));
    assert_eq!(
        attr(&span, "http.response.status_code").as_deref(),
        Some("200")
    );
    assert_eq!(
        attr(&span, "error.type").as_deref(),
        Some("invalid_response")
    );
    assert!(matches!(span.status, Status::Error { .. }));
}
