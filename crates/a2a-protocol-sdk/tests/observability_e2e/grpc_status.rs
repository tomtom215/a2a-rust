// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which gRPC statuses are errors on a `SERVER` span. `docs/rpc/grpc.md`
//! (`open-telemetry/semantic-conventions@55c98cf`), server span: "The
//! following status codes SHOULD be considered errors: `UNKNOWN`,
//! `DEADLINE_EXCEEDED`, `UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE`,
//! `DATA_LOSS`". So a `GetTask` for a task that does not exist — the
//! caller's mistake, answered correctly — is `rpc.status_code` `NOT_FOUND`
//! with no `error.type` and its status left unset, and one the store failed
//! is `INTERNAL`, with both.

use std::collections::BTreeMap;

use a2a_protocol_sdk::client::A2aClient;
use a2a_protocol_sdk::types::params::TaskQueryParams;
use opentelemetry::trace::{SpanKind, Status};
use opentelemetry_sdk::trace::SpanData;

use crate::GET_TASK;
use crate::fixtures::{STORE_DOWN, attr};

/// A `GetTask` for a task that does not exist, then one whose read fails.
pub async fn run(client: &A2aClient) {
    for id in ["no-such-task", STORE_DOWN] {
        let _ = client
            .get_task(TaskQueryParams {
                tenant: None,
                id: id.to_owned(),
                history_length: None,
            })
            .await
            .expect_err("both fail");
    }
}

/// The two calls' spans.
pub fn check_spans(spans: &[SpanData], gaps: &mut Vec<String>) {
    for (status, error) in [("NOT_FOUND", false), ("INTERNAL", true)] {
        let span = spans.iter().find(|s| {
            s.span_kind == SpanKind::Server
                && s.name == GET_TASK
                && attr(s, "rpc.system.name").as_deref() == Some("grpc")
                && attr(s, "rpc.status_code").as_deref() == Some(status)
        });
        let Some(span) = span else {
            gaps.push(format!(
                "gRPC: no `{GET_TASK}` SERVER span with rpc.status_code {status}"
            ));
            continue;
        };
        let error_type = attr(span, "error.type");
        let marked = matches!(span.status, Status::Error { .. });
        let want = error.then(|| status.to_owned());
        if error_type != want || marked != error {
            gaps.push(format!(
                "gRPC: the {status} GetTask span has error.type {error_type:?} and status {:?}; \
                 want error.type {want:?} and {}",
                span.status,
                if error { "Error" } else { "Unset" }
            ));
        }
    }
}

/// The two calls' `rpc.server.call.duration` points.
pub fn check_points(points: &[BTreeMap<String, String>], gaps: &mut Vec<String>) {
    for (status, error) in [("NOT_FOUND", None), ("INTERNAL", Some("INTERNAL"))] {
        let found = points.iter().any(|p| {
            p.get("rpc.system.name").map(String::as_str) == Some("grpc")
                && p.get("rpc.method").map(String::as_str) == Some(GET_TASK)
                && p.get("rpc.status_code").map(String::as_str) == Some(status)
                && p.get("error.type").map(String::as_str) == error
        });
        if !found {
            gaps.push(format!(
                "gRPC: no rpc.server.call.duration point for GetTask with rpc.status_code \
                 {status} and error.type {error:?}; points: {points:?}"
            ));
        }
    }
}
