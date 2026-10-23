// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The push delivery as an operator sees it: the webhook POST runs in an
//! HTTP `CLIENT` span (`docs/http/http-spans.md`: named `POST`, with
//! `http.request.method`, `server.address`, `url.full` and the response
//! status), and the webhook receives that span as its `traceparent`, so a
//! receiver that continues the trace hangs off a span the backend has, in
//! the trace of the call that configured the push.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_sdk::client::A2aClient;
use a2a_protocol_sdk::types::params::SendMessageConfiguration;
use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;
use a2a_protocol_sdk::types::trace_context::TraceContext;
use opentelemetry::trace::SpanKind;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};

use crate::fixtures::{WEBHOOK_TRACEPARENTS, attr, message, webhook};

/// The streaming send that configured the push.
const STREAM_SPAN: &str = "lf.a2a.v1.A2AService/SendStreamingMessage";

/// A streaming send that configures a push to a local webhook, and waits —
/// with a deadline, not for a fixed time (CONTRIBUTING.md) — for the
/// delivery that runs behind the stream to reach it.
pub async fn deliver(client: &A2aClient) {
    let hits = Arc::new(AtomicUsize::new(0));
    let hook = webhook(Arc::clone(&hits)).await;
    let mut push = message("push", None);
    push.configuration = Some(SendMessageConfiguration {
        task_push_notification_config: Some(TaskPushNotificationConfig::new("", hook)),
        ..SendMessageConfiguration::default()
    });
    let mut stream = client.stream_message(push).await.expect("push stream");
    while tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .ok()
        .flatten()
        .is_some()
    {}
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while hits.load(Ordering::SeqCst) == 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// Checks the webhook's `traceparent` against the exported spans, waiting
/// with a deadline for the span it names: a span is exported when it
/// closes, which for the POST's may be just after the webhook answered.
pub async fn check_webhook(
    exporter: &InMemorySpanExporter,
    provider: &SdkTracerProvider,
    gaps: &mut Vec<String>,
) {
    let received = WEBHOOK_TRACEPARENTS.lock().expect("lock").clone();
    let Some(first) = received.first() else {
        gaps.push("push: the webhook received no POST".to_owned());
        return;
    };
    let Some(traceparent) = first else {
        gaps.push("push: the webhook POST carried no traceparent".to_owned());
        return;
    };
    let Ok(sent) = TraceContext::parse(traceparent) else {
        gaps.push(format!(
            "push: the webhook's traceparent {traceparent:?} is malformed"
        ));
        return;
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let (spans, named) = loop {
        let _ = provider.force_flush();
        let spans = exporter.get_finished_spans().expect("spans");
        let named = spans.iter().position(|s| {
            s.span_context.span_id().to_string() == sent.span_id()
                && s.span_context.trace_id().to_string() == sent.trace_id()
        });
        if named.is_some() || tokio::time::Instant::now() >= deadline {
            break (spans, named);
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    };
    let Some(post) = named.map(|i| &spans[i]) else {
        gaps.push(format!(
            "push: the webhook's traceparent {traceparent} names no exported span"
        ));
        return;
    };
    check_post(post, gaps);
    let in_call_trace = spans.iter().any(|s: &SpanData| {
        s.span_kind == SpanKind::Server
            && s.name == STREAM_SPAN
            && s.span_context.trace_id() == post.span_context.trace_id()
    });
    if !in_call_trace {
        gaps.push(format!(
            "push: the webhook POST's span is not in the trace of the `{STREAM_SPAN}` that configured it"
        ));
    }
}

/// The POST's span follows the HTTP client conventions.
fn check_post(post: &SpanData, gaps: &mut Vec<String>) {
    if post.span_kind != SpanKind::Client || post.name != "POST" {
        gaps.push(format!(
            "push: the webhook POST's span is {:?} `{}`, not CLIENT `POST`",
            post.span_kind, post.name
        ));
    }
    for (key, want) in [
        ("http.request.method", "POST"),
        ("server.address", "127.0.0.1"),
        ("http.response.status_code", "200"),
    ] {
        if attr(post, key).as_deref() != Some(want) {
            gaps.push(format!(
                "push: the webhook POST's span has {key} {:?}, not {want:?}",
                attr(post, key)
            ));
        }
    }
    // The webhook's path is its receiver's secret: kept out of `url.full`.
    if !attr(post, "url.full").is_some_and(|u| u.ends_with("/REDACTED") && !u.contains("hook")) {
        gaps.push(format!(
            "push: the webhook POST's url.full is {:?}, not its origin and `/REDACTED`",
            attr(post, "url.full")
        ));
    }
}
