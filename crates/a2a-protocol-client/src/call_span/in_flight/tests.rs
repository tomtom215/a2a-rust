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
