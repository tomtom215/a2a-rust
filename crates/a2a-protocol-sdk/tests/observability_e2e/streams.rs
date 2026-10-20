// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A streaming call lasts as long as its stream, on every binding.
//!
//! `docs/rpc/rpc-spans.md` (`open-telemetry/semantic-conventions@55c98cf`):
//! "For streaming RPCs, the span SHOULD cover the full lifetime of the
//! request and/or response streams until they are closed or terminated";
//! `rpc-metrics.md`: the call's duration "SHOULD be the same as the RPC
//! server span duration". An agent that works for [`HOLD`] before it
//! finishes answers each binding's `SendStreamingMessage`; the call's
//! `SERVER` span and its `rpc.server.call.duration` must last at least that
//! long, and the client's `rpc.client.call.duration` too. A gRPC stream the
//! client drops after its first event is the call cancelled.
//!
//! With the `websocket` feature it also checks what a WebSocket call's span
//! is a child of: a frame carries no trace context, and the upgrade
//! request's `traceparent` names the operation that opened the connection,
//! so each call is linked to it, not parented by it — and each call is a
//! span of its own.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::client::{A2aClient, ClientBuilder};
use a2a_protocol_sdk::server::{
    EventEmitter, GrpcConfig, GrpcDispatcher, JsonRpcDispatcher, RequestHandlerBuilder,
    RestDispatcher, agent_executor, serve_with_addr,
};
use a2a_protocol_sdk::types::{
    AgentCapabilities, AgentCard, AgentInterface, AgentSkill, TaskState,
};
use opentelemetry::trace::SpanKind;
use opentelemetry_sdk::trace::SpanData;

use crate::fixtures::{attr, message};

/// How long the agent works before it finishes.
pub const HOLD: Duration = Duration::from_millis(150);

const STREAM: &str = "lf.a2a.v1.A2AService/SendStreamingMessage";
const SEND: &str = "lf.a2a.v1.A2AService/SendMessage";

/// The trace a WebSocket upgrade request carries.
#[cfg(feature = "websocket")]
const UPGRADE_TRACE: &str = "3df7651916cd43dd8448eb211c80319c";
#[cfg(feature = "websocket")]
const UPGRADE_SPAN: &str = "e7ad6b7169203331";

struct Slow;

agent_executor!(Slow, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    tokio::time::sleep(HOLD).await;
    emit.status(TaskState::Completed).await
});

/// The bindings whose streams are checked: the binding, its
/// `rpc.system.name`, and the message text that marks its call.
const STREAMED: [(&str, &str); 3] = [
    ("jsonrpc", "stream-JSONRPC"),
    ("a2a_http_json", "stream-HTTP+JSON"),
    ("grpc", "stream-GRPC"),
];

/// Streams one slow task over each binding, cancels one gRPC stream, and,
/// with the `websocket` feature, makes three calls on one connection.
pub async fn run() {
    let card = AgentCard::new("slow", "1.0.0", AgentInterface::jsonrpc("http://127.0.0.1"))
        .with_description("An agent that takes its time")
        .with_skill(AgentSkill::new("slow", "Slow", "Works, then finishes").with_tags(["slow"]))
        .with_capabilities(AgentCapabilities::none().with_streaming(true));
    let handler = Arc::new(
        RequestHandlerBuilder::new(Slow)
            .with_agent_card(card)
            .with_metrics(a2a_protocol_sdk::server::otel::OtelMetricsBuilder::new().build())
            .build()
            .expect("handler"),
    );
    let jsonrpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("jsonrpc");
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("rest");
    let grpc = GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default())
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("grpc");
    let client = |binding: &str, url: String| {
        ClientBuilder::new(url)
            .with_protocol_binding(binding)
            .build()
            .expect("client")
    };
    let grpc_client = ClientBuilder::new(format!("http://{grpc}"))
        .with_protocol_binding("GRPC")
        .build_grpc()
        .await
        .expect("grpc client");
    let clients = [
        client("JSONRPC", format!("http://{jsonrpc}")),
        client("HTTP+JSON", format!("http://{rest}")),
        grpc_client,
    ];
    for (client, (_, text)) in clients.iter().zip(STREAMED) {
        drain(client, text).await;
    }

    // The client goes away after the first event, while the agent works.
    let mut stream = clients[2]
        .stream_message(message("stream-cancel", None))
        .await
        .expect("stream");
    let _ = stream.next().await;
    drop(stream);
    tokio::time::sleep(HOLD * 2).await;

    #[cfg(feature = "websocket")]
    websocket(&handler).await;
}

async fn drain(client: &A2aClient, text: &str) {
    let mut stream = client
        .stream_message(message(text, None))
        .await
        .unwrap_or_else(|e| panic!("{text}: {e}"));
    while let Some(event) = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .ok()
        .flatten()
    {
        event.unwrap_or_else(|e| panic!("{text}: {e}"));
    }
}

/// Two unary calls and one stream on one WebSocket connection, opened with
/// a `traceparent`.
#[cfg(feature = "websocket")]
async fn websocket(handler: &Arc<a2a_protocol_sdk::server::RequestHandler>) {
    use a2a_protocol_sdk::client::WebSocketTransport;
    use a2a_protocol_sdk::server::WebSocketDispatcher;

    let addr = Arc::new(WebSocketDispatcher::new(Arc::clone(handler)))
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("websocket");
    let headers = std::collections::HashMap::from([(
        "traceparent".to_owned(),
        format!("00-{UPGRADE_TRACE}-{UPGRADE_SPAN}-01"),
    )]);
    let transport = WebSocketTransport::connect_with_options(
        format!("ws://{addr}"),
        Duration::from_secs(10),
        &headers,
    )
    .await
    .expect("connect");
    let client = ClientBuilder::new(format!("ws://{addr}"))
        .with_custom_transport(transport)
        .build()
        .expect("client");
    for text in ["ws-1", "ws-2"] {
        client
            .send_message(message(text, None))
            .await
            .unwrap_or_else(|e| panic!("{text}: {e}"));
    }
    drain(&client, "stream-WEBSOCKET").await;
}

/// The `SERVER` span of the call whose message was `text`.
fn server_span<'a>(spans: &'a [SpanData], text: &str) -> Option<&'a SpanData> {
    let id = format!("m-{text}");
    spans.iter().find(|s| {
        s.span_kind == SpanKind::Server && attr(s, "a2a.message.id").as_deref() == Some(&id)
    })
}

fn lasted(span: &SpanData) -> Duration {
    span.end_time
        .duration_since(span.start_time)
        .unwrap_or_default()
}

pub fn check_spans(spans: &[SpanData], gaps: &mut Vec<String>) {
    let mut streamed: Vec<(&str, &str)> = STREAMED.to_vec();
    if cfg!(feature = "websocket") {
        streamed.push(("jsonrpc", "stream-WEBSOCKET"));
    }
    for (system, text) in streamed {
        let Some(span) = server_span(spans, text) else {
            gaps.push(format!("{text}: no SERVER span"));
            continue;
        };
        if span.name != STREAM || attr(span, "rpc.system.name").as_deref() != Some(system) {
            gaps.push(format!(
                "{text}: the SERVER span is `{}` on {:?}",
                span.name,
                attr(span, "rpc.system.name")
            ));
        }
        if lasted(span) < HOLD {
            gaps.push(format!(
                "{text}: the SERVER span lasted {:?}, ending before its stream, which lasted at \
                 least {HOLD:?}",
                lasted(span)
            ));
        }
        if let Some(error) = attr(span, "error.type") {
            gaps.push(format!("{text}: a completed stream has error.type {error}"));
        }
    }
    match server_span(spans, "stream-cancel") {
        None => gaps.push("stream-cancel: no SERVER span".to_owned()),
        Some(span) => {
            if attr(span, "error.type").as_deref() != Some("cancelled") {
                gaps.push(format!(
                    "stream-cancel: a stream its client dropped has error.type {:?}, not \
                     \"cancelled\"",
                    attr(span, "error.type")
                ));
            }
        }
    }
    #[cfg(feature = "websocket")]
    check_websocket(spans, gaps);
}

/// Each WebSocket call is a span of its own, none the upgrade's child, each
/// linked to it.
#[cfg(feature = "websocket")]
fn check_websocket(spans: &[SpanData], gaps: &mut Vec<String>) {
    let mut ids = std::collections::HashSet::new();
    for text in ["ws-1", "ws-2", "stream-WEBSOCKET"] {
        let Some(span) = server_span(spans, text) else {
            gaps.push(format!("{text}: no SERVER span"));
            continue;
        };
        ids.insert(span.span_context.span_id());
        if span.parent_span_id.to_string() == UPGRADE_SPAN
            || span.span_context.trace_id().to_string() == UPGRADE_TRACE
        {
            gaps.push(format!(
                "{text}: the WebSocket call's span is a child of the upgrade request's trace"
            ));
        }
        let linked = span.links.iter().any(|l| {
            l.span_context.trace_id().to_string() == UPGRADE_TRACE
                && l.span_context.span_id().to_string() == UPGRADE_SPAN
        });
        if !linked {
            gaps.push(format!(
                "{text}: the WebSocket call's span has no link to the upgrade request's trace"
            ));
        }
    }
    if ids.len() != 3 {
        gaps.push(format!(
            "three WebSocket calls recorded {} distinct SERVER spans",
            ids.len()
        ));
    }
}

/// `rpc.server.call.duration` and `rpc.client.call.duration` both cover each
/// binding's stream: the longest recorded streaming call lasted at least
/// [`HOLD`].
pub fn check_durations(
    server: &[(BTreeMap<String, String>, f64)],
    client: &[(BTreeMap<String, String>, f64)],
    gaps: &mut Vec<String>,
) {
    for (instrument, points) in [
        ("rpc.server.call.duration", server),
        ("rpc.client.call.duration", client),
    ] {
        for (system, _) in STREAMED {
            let longest = points
                .iter()
                .filter(|(p, _)| {
                    p.get("rpc.system.name").map(String::as_str) == Some(system)
                        && p.get("rpc.method").map(String::as_str) == Some(STREAM)
                        && !p.contains_key("error.type")
                })
                .map(|(_, max)| *max)
                .fold(0.0_f64, f64::max);
            if longest < HOLD.as_secs_f64() {
                gaps.push(format!(
                    "{instrument}: the longest {system} stream lasted {longest} s, ending \
                     before its stream, which lasted at least {HOLD:?}"
                ));
            }
        }
    }
}

/// `rpc.client.call.duration` carries the conventions' attributes: each
/// binding's send, recorded as a success, with the server it reached; and
/// gRPC's `NOT_FOUND` `GetTask` with that status as `rpc.status_code` and
/// `error.type`, as on the client every failed status is an error.
pub fn check_client_points(points: &[BTreeMap<String, String>], gaps: &mut Vec<String>) {
    let get = |p: &BTreeMap<String, String>, k: &str| p.get(k).cloned();
    for system in ["jsonrpc", "a2a_http_json", "grpc"] {
        let found = points.iter().any(|p| {
            get(p, "rpc.system.name").as_deref() == Some(system)
                && get(p, "rpc.method").as_deref() == Some(SEND)
                && get(p, "server.address").as_deref() == Some("127.0.0.1")
                && p.contains_key("server.port")
                && !p.contains_key("error.type")
        });
        if !found {
            gaps.push(format!(
                "rpc.client.call.duration: no successful {system} {SEND} point with \
                 server.address and server.port; points: {points:?}"
            ));
        }
    }
    let found = points.iter().any(|p| {
        get(p, "rpc.system.name").as_deref() == Some("grpc")
            && get(p, "rpc.method").as_deref() == Some(crate::GET_TASK)
            && get(p, "rpc.status_code").as_deref() == Some("NOT_FOUND")
            && get(p, "error.type").as_deref() == Some("NOT_FOUND")
    });
    if !found {
        gaps.push(format!(
            "rpc.client.call.duration: no gRPC GetTask point with rpc.status_code and \
             error.type NOT_FOUND; points: {points:?}"
        ));
    }
}
