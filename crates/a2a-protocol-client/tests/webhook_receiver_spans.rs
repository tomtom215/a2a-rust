// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `WebhookReceiver::receive`'s span as exported: where it sits in the
//! trace, and what it records of each outcome.

#![cfg(all(feature = "webhook", feature = "otel"))]

use std::sync::{Arc, Mutex};

use a2a_protocol_client::webhook::{WebhookReceiver, new_notification_token};
use a2a_protocol_types::{
    ContextId, StreamResponse, TaskId, TaskPushNotificationConfig, TaskState, TaskStatus,
    TaskStatusUpdateEvent,
};
use opentelemetry::trace::{SpanKind, Status, TracerProvider as _};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanExporter};
use tracing_subscriber::layer::SubscriberExt as _;

const TASK: &str = "task-1";
/// A sender's span: what `HttpPushSender` puts in its POST's `traceparent`.
const SENDER_TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const SENDER_SPAN: &str = "00f067aa0ba902b7";

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

/// Runs `f` with every span exported to the returned list.
fn exported(f: impl FnOnce()) -> Vec<SpanData> {
    let keep = Keep::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(keep.clone())
        .build();
    let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("webhook-test"));
    tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), f);
    std::mem::take(&mut *keep.0.lock().unwrap())
}

fn receiver(token: &str, credentials: &str) -> WebhookReceiver {
    let mut config = TaskPushNotificationConfig::new(TASK, "https://hooks.example.com/a2a");
    config.id = Some("c1".to_owned());
    config.token = Some(token.to_owned());
    config.authentication = Some(a2a_protocol_types::push::AuthenticationInfo {
        scheme: "Bearer".into(),
        credentials: Some(credentials.into()),
    });
    let receiver = WebhookReceiver::new();
    receiver.register(&config).unwrap();
    receiver
}

fn headers(token: &str, authorization: &str, traceparent: bool) -> hyper::HeaderMap {
    let mut h = hyper::HeaderMap::new();
    h.insert("a2a-notification-token", token.parse().unwrap());
    h.insert("authorization", authorization.parse().unwrap());
    if traceparent {
        h.insert(
            "traceparent",
            format!("00-{SENDER_TRACE}-{SENDER_SPAN}-01")
                .parse()
                .unwrap(),
        );
    }
    h
}

fn body() -> Vec<u8> {
    serde_json::to_vec(&StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new(TASK),
        context_id: ContextId::new("ctx"),
        status: TaskStatus::new(TaskState::Working),
        metadata: None,
    }))
    .unwrap()
}

fn attr(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

fn receive_span(spans: &[SpanData]) -> &SpanData {
    spans
        .iter()
        .find(|s| s.name == "a2a.webhook.receive")
        .unwrap_or_else(|| panic!("no receive span in {spans:?}"))
}

/// With no recorded span current, the receive span continues the trace the
/// sender's POST named, so the notification is found under its task.
#[test]
fn a_request_with_a_traceparent_continues_the_senders_trace() {
    let token = new_notification_token();
    let receiver = receiver(&token, "s3cr3t");
    let spans = exported(|| {
        receiver.receive(&headers(&token, "Bearer s3cr3t", true), &body());
    });
    let span = receive_span(&spans);
    assert_eq!(span.span_kind, SpanKind::Internal);
    assert_eq!(span.span_context.trace_id().to_string(), SENDER_TRACE);
    assert_eq!(span.parent_span_id.to_string(), SENDER_SPAN);
    assert!(span.parent_span_is_remote);
}

/// A span the integrator's server recorded wins: the receive span is its
/// child, whatever the request says.
#[test]
fn a_recorded_current_span_is_the_parent_instead() {
    let token = new_notification_token();
    let receiver = receiver(&token, "s3cr3t");
    let spans = exported(|| {
        let server = tracing::info_span!("http.server");
        let _entered = server.enter();
        receiver.receive(&headers(&token, "Bearer s3cr3t", true), &body());
    });
    let server = spans.iter().find(|s| s.name == "http.server").unwrap();
    let span = receive_span(&spans);
    assert_eq!(span.parent_span_id, server.span_context.span_id());
    assert_ne!(span.span_context.trace_id().to_string(), SENDER_TRACE);
}

/// An accepted request records its outcome and task; a rejected one its
/// kind as `error.type` and `Error` status. Neither records the token or the
/// credentials.
#[test]
fn the_outcome_is_recorded_and_no_secret_is() {
    let token = new_notification_token();
    let receiver = receiver(&token, "s3cr3t");
    let spans = exported(|| {
        receiver.receive(&headers(&token, "Bearer s3cr3t", false), &body());
        receiver.receive(&headers(&token, "Bearer wrong", false), &body());
    });
    let receives: Vec<_> = spans
        .iter()
        .filter(|s| s.name == "a2a.webhook.receive")
        .collect();
    assert_eq!(receives.len(), 2);
    let accepted = receives[0];
    assert_eq!(
        attr(accepted, "a2a.webhook.outcome").as_deref(),
        Some("accepted")
    );
    assert_eq!(attr(accepted, "a2a.task.id").as_deref(), Some(TASK));
    assert_eq!(attr(accepted, "error.type"), None);
    let rejected = receives[1];
    assert_eq!(
        attr(rejected, "a2a.webhook.outcome").as_deref(),
        Some("rejected")
    );
    assert_eq!(
        attr(rejected, "error.type").as_deref(),
        Some("bad_authorization")
    );
    assert!(matches!(rejected.status, Status::Error { .. }));
    assert_eq!(attr(rejected, "a2a.task.id"), None, "not authenticated");
    for span in &receives {
        for kv in &span.attributes {
            let value = kv.value.to_string();
            assert!(
                !value.contains(&token) && !value.contains("s3cr3t"),
                "{} carries a secret",
                kv.key
            );
        }
    }
}
