// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A call the peer abandons is not a server failure (audit C-09; ADR 0038
//! §2): no binding marks its span `Error`; gRPC records the status
//! `CANCELLED` and no `error.type` (semantic conventions `docs/rpc/grpc.md`,
//! server span: only `UNKNOWN`, `DEADLINE_EXCEEDED`, `UNIMPLEMENTED`,
//! `INTERNAL`, `UNAVAILABLE` and `DATA_LOSS` "SHOULD be considered errors");
//! the bindings with no status for an unanswered call keep `error.type`
//! `cancelled` (`docs/rpc/rpc-spans.md`, `error.type`: "If the RPC fails with
//! an error before status code is returned … a component-specific, low
//! cardinality error identifier").

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::error::ServerError;
use crate::metrics::{Metrics, RpcCall};

/// `(rpc.status_code, error.type)` of each call recorded.
type Statuses = Arc<std::sync::Mutex<Vec<(Option<String>, Option<String>)>>>;

struct Recording(Statuses);

impl Metrics for Recording {
    fn on_rpc_call(&self, call: &RpcCall<'_>) {
        self.0.lock().unwrap().push((
            call.status_code.map(str::to_owned),
            call.error_type.map(str::to_owned),
        ));
    }
}

fn handler(seen: &Statuses) -> RequestHandler {
    struct Idle;
    crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
    crate::RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_metrics(Recording(Arc::clone(seen)))
        .build()
        .unwrap()
}

/// Opens a call on `system`, polls it once, and drops it unfinished.
async fn abandon(handler: &RequestHandler, system: RpcSystem) {
    let call = ServerSpan::open(handler, system, "GetTask", None)
        .run(std::future::pending::<Result<(), ServerError>>());
    assert!(
        tokio::time::timeout(Duration::from_millis(10), call)
            .await
            .is_err()
    );
}

fn owned(s: &str) -> String {
    s.to_owned()
}

#[tokio::test]
async fn an_abandoned_call_is_cancelled_on_grpc_and_typed_cancelled_elsewhere() {
    let seen = Statuses::default();
    let handler = handler(&seen);
    let custom = CustomBinding::new("custom", |_| "x".into());
    for system in [
        RpcSystem::Grpc,
        RpcSystem::JsonRpc,
        RpcSystem::HttpJson,
        RpcSystem::Custom(custom),
        RpcSystem::Custom(custom.with_grpc_server_errors()),
    ] {
        abandon(&handler, system).await;
    }
    assert_eq!(
        *seen.lock().unwrap(),
        [
            (Some(owned("CANCELLED")), None),
            (None, Some(owned("cancelled"))),
            (None, Some(owned("cancelled"))),
            (None, Some(owned("cancelled"))),
            (None, Some(owned("cancelled"))),
        ]
    );
}

/// The fields one call's span ended with, under a recording subscriber.
#[cfg(feature = "tracing")]
fn span_fields(
    system: RpcSystem,
    outcome: Option<ServerError>,
) -> std::collections::BTreeMap<String, String> {
    use tracing_subscriber::layer::SubscriberExt as _;
    let fields = super::tests::Fields::default();
    let handler = handler(&Statuses::default());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(fields.clone()), || {
        runtime.block_on(async {
            match outcome {
                Some(err) => {
                    let _ = ServerSpan::open(&handler, system, "GetTask", None)
                        .run(async { Err::<(), _>(err) })
                        .await;
                }
                None => abandon(&handler, system).await,
            }
        });
    });
    fields.0.lock().unwrap().clone()
}

#[cfg(feature = "tracing")]
#[test]
fn an_abandoned_calls_span_is_not_an_error_on_any_binding() {
    let grpc = span_fields(RpcSystem::Grpc, None);
    assert_eq!(
        grpc.get("rpc.status_code").map(String::as_str),
        Some("CANCELLED")
    );
    assert!(!grpc.contains_key("error.type"), "{grpc:?}");
    assert!(!grpc.contains_key("otel.status_code"), "{grpc:?}");
    for system in [RpcSystem::JsonRpc, RpcSystem::HttpJson] {
        let got = span_fields(system, None);
        assert_eq!(
            got.get("error.type").map(String::as_str),
            Some("cancelled"),
            "{got:?}"
        );
        assert!(!got.contains_key("otel.status_code"), "{got:?}");
        assert!(
            !got.contains_key("rpc.status_code"),
            "no status was sent: {got:?}"
        );
    }
}

/// A call that failed on the server still marks its span `Error`.
#[cfg(feature = "tracing")]
#[test]
fn a_server_failure_still_marks_the_span_an_error() {
    for system in [RpcSystem::JsonRpc, RpcSystem::HttpJson] {
        let got = span_fields(system, Some(ServerError::Internal("broke".into())));
        assert_eq!(
            got.get("otel.status_code").map(String::as_str),
            Some("ERROR"),
            "{got:?}"
        );
        assert!(got.contains_key("error.type"), "{got:?}");
    }
}
