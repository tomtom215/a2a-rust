// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a call's span records of how it ended; the same ends measured, with
//! real calls, are in `call_metric/tests.rs`.

#![cfg(feature = "tracing")]

use std::collections::BTreeMap;

use super::InFlight;
use crate::ClientBuilder;
use crate::call_span::tests::recorded;
use crate::error::ClientError;

/// What a call's span records when `act` ends it.
fn recorded_by(act: impl FnOnce(InFlight<'_>)) -> BTreeMap<String, String> {
    let client = ClientBuilder::new("http://agent.example:9000")
        .build()
        .expect("client");
    recorded(|| {
        act(InFlight::open(
            &client,
            "SendMessage",
            &serde_json::json!({}),
        ));
    })
}

fn field<'a>(fields: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    fields.get(key).map(String::as_str)
}

#[test]
fn a_call_dropped_unfinished_ends_its_span_cancelled() {
    let fields = recorded_by(|call| drop(call));
    assert_eq!(
        field(&fields, "error.type"),
        Some("cancelled"),
        "{fields:?}"
    );
    assert_eq!(field(&fields, "otel.status_code"), Some("ERROR"));
    assert!(!fields.contains_key("rpc.status_code"), "{fields:?}");
}

#[test]
fn a_call_that_ended_well_is_not_recorded_as_cancelled() {
    let fields = recorded_by(|call| call.end(None));
    assert!(!fields.contains_key("error.type"), "{fields:?}");
    assert!(!fields.contains_key("otel.status_code"), "{fields:?}");
}

/// A response the client then refuses is a failure of the call, with the
/// class of the refusal.
#[test]
fn a_call_whose_response_was_refused_ends_its_span_with_the_refusal() {
    for (error, class) in [
        (
            ClientError::InvalidAgentResponse("no id".into()),
            "invalid_response",
        ),
        (
            ClientError::ContentNotVerified("bad".into()),
            "content_not_verified",
        ),
        (ClientError::Timeout("slow".into()), "timeout"),
    ] {
        let fields = recorded_by(|call| call.end(Some(&error)));
        assert_eq!(field(&fields, "error.type"), Some(class), "{fields:?}");
        assert_eq!(field(&fields, "otel.status_code"), Some("ERROR"));
    }
}

/// What the agent answers of its task reaches the call's span, through the
/// call a client holds (the span's own test reads `CallSpan::response`).
#[test]
fn the_call_records_what_the_agent_answers_of_its_task() {
    let fields = recorded_by(|mut call| {
        call.response(&serde_json::json!({"task": {
            "id": "t1",
            "contextId": "c1",
            "status": {"state": "TASK_STATE_COMPLETED"}
        }}));
        call.end(None);
    });
    assert_eq!(
        field(&fields, "a2a.task.state"),
        Some("TASK_STATE_COMPLETED"),
        "{fields:?}"
    );
    assert_eq!(field(&fields, "a2a.task.id"), Some("t1"), "{fields:?}");
}

/// The call writes its own span's `traceparent` into the request's headers,
/// through the call a client holds (the free function has its own tests).
#[cfg(feature = "otel")]
#[test]
fn the_call_writes_a_traceparent_naming_its_own_span() {
    use opentelemetry::trace::TraceContextExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;

    let client = ClientBuilder::new("http://agent.example:9000")
        .build()
        .expect("client");
    let (headers, span_context) = crate::call_span::tests::with_otel(|| {
        let call = InFlight::open(&client, "SendMessage", &serde_json::json!({}));
        let mut headers = std::collections::HashMap::new();
        call.propagate(&mut headers);
        let context = call.call.span.context().span().span_context().clone();
        (headers, context)
    });
    assert!(span_context.is_valid());
    let sent = crate::call_span::tests::traceparent(&headers);
    assert_eq!(sent.trace_id(), span_context.trace_id().to_string());
    assert_eq!(sent.span_id(), span_context.span_id().to_string());
}

/// A client built to keep its trace ids from the agent sends none.
#[cfg(feature = "otel")]
#[test]
fn a_client_that_does_not_propagate_writes_no_traceparent() {
    let client = ClientBuilder::new("http://agent.example:9000")
        .with_trace_propagation(false)
        .build()
        .expect("client");
    let headers = crate::call_span::tests::with_otel(|| {
        let call = InFlight::open(&client, "SendMessage", &serde_json::json!({}));
        let mut headers = std::collections::HashMap::new();
        call.propagate(&mut headers);
        headers
    });
    assert!(headers.is_empty(), "{headers:?}");
}
