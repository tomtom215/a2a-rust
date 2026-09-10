// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Each act as a test, plus the fault injectors on their own.
//!
//! The act tests run the same code `cargo run` does and require a `Pass`, so
//! the README's transcript and the test suite cannot drift apart. The
//! injector tests exist because an injector that does not inject makes every
//! act vacuous — a webhook that never refuses would let the push check "pass"
//! with all deliveries succeeding.

use std::sync::Arc;
use std::time::Duration;

use hyper::StatusCode;

use crate::support::executors::FlakyTally;
use crate::support::injectors::{Fault, faulting_proxy, webhook_sink};
use crate::support::metrics::RecordingMetrics;
use crate::{Check, Outcome};

fn require_pass(check: &Check) {
    match &check.outcome {
        Outcome::Pass(detail) => println!("[ok] {}: {detail}", check.label),
        Outcome::Fail(detail) => panic!("{} failed: {detail}", check.label),
        Outcome::NotCompiled(feature) => {
            panic!(
                "{} is compiled out; run with --features {feature}",
                check.label
            );
        }
        Outcome::NotRun(reason) => panic!("{} did not run: {reason}", check.label),
    }
}

// ── Act 1 ───────────────────────────────────────────────────────────────────

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn act_1_durability_across_a_handler_restart() {
    for check in crate::durability::run().await {
        require_pass(&check);
    }
}

// ── Act 2 ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn act_2_failure_injection() {
    for check in crate::failure::run().await {
        require_pass(&check);
    }
}

// ── Act 3 ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn act_3_in_memory_replicas_do_not_share_tasks() {
    let checks = crate::scaling::run().await;
    require_pass(&checks[0]);
}

/// Passes when PostgreSQL is configured and the shared checks hold; when it
/// is not configured, prints the honest `[NOT RUN]` and returns rather than
/// pretending — run with `--nocapture` to see it. A configured-but-broken
/// server is a failure, as in `incident-response`.
#[cfg(feature = "postgres")]
#[tokio::test]
async fn act_3_shared_postgres_store_and_counter() {
    let checks = crate::scaling::run().await;
    for check in &checks[1..] {
        match &check.outcome {
            Outcome::NotRun(reason) => {
                eprintln!("[NOT RUN] {}: {reason}", check.label);
            }
            _ => require_pass(check),
        }
    }
}

// ── The injectors ───────────────────────────────────────────────────────────

#[tokio::test]
async fn webhook_sink_refuses_exactly_m_then_accepts() {
    let (url, tally) = webhook_sink(2, StatusCode::SERVICE_UNAVAILABLE).await;
    let client: hyper_util::client::legacy::Client<_, http_body_util::Full<bytes::Bytes>> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build_http();
    let mut statuses = Vec::new();
    for _ in 0..4 {
        let req = hyper::Request::post(&url)
            .body(http_body_util::Full::new(bytes::Bytes::from_static(b"{}")))
            .expect("request builds");
        statuses.push(client.request(req).await.expect("sink answers").status());
    }
    assert_eq!(
        statuses,
        [
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::OK,
            StatusCode::OK
        ]
    );
    assert_eq!((tally.refused(), tally.accepted()), (2, 2));
}

#[tokio::test]
async fn faulting_proxy_drops_exactly_k_connections() {
    // Upstream is irrelevant: the first K connections never reach it.
    let (url, tally) =
        faulting_proxy("http://127.0.0.1:9/".to_owned(), 2, Fault::DropConnection).await;
    let client: hyper_util::client::legacy::Client<_, http_body_util::Full<bytes::Bytes>> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build_http();
    let mut outcomes = Vec::new();
    for _ in 0..3 {
        let req = hyper::Request::post(&url)
            .body(http_body_util::Full::new(bytes::Bytes::from_static(b"{}")))
            .expect("request builds");
        outcomes.push(client.request(req).await.map(|r| r.status()));
    }
    assert!(outcomes[0].is_err(), "first connection should be dropped");
    assert!(outcomes[1].is_err(), "second connection should be dropped");
    // The third is forwarded to a dead upstream: a 502 from the proxy, which
    // proves it got past the fault and tried.
    assert_eq!(
        outcomes[2].as_ref().expect("third is answered"),
        &StatusCode::BAD_GATEWAY
    );
    assert_eq!((tally.faulted(), tally.forwarded()), (2, 1));
}

#[test]
fn flaky_tally_starts_with_no_invocations() {
    let tally = Arc::new(FlakyTally::failing_first(3));
    assert_eq!(tally.invocations(), 0);
}

#[tokio::test]
async fn recording_metrics_counts_and_waits() {
    use a2a_protocol_server::metrics::Metrics as _;
    let metrics = RecordingMetrics::default();
    metrics.on_push_delivery("failed");
    metrics.on_push_delivery("delivered");
    metrics.on_push_delivery("delivered");
    metrics.on_persistence_error("status_update", "internal");
    assert_eq!(
        metrics
            .wait_for_push_outcomes(3, Duration::from_millis(10))
            .await,
        3
    );
    let counts = metrics.push_counts();
    assert_eq!(counts.get("delivered"), Some(&2));
    assert_eq!(counts.get("failed"), Some(&1));
    assert_eq!(
        crate::support::metrics::render_counts(&counts),
        "delivered=2, failed=1"
    );
    assert_eq!(
        metrics.persistence_errors(),
        vec![("status_update".to_owned(), "internal".to_owned())]
    );
    // A budget that runs out reports what it saw, not what was asked for.
    assert_eq!(
        metrics
            .wait_for_push_outcomes(9, Duration::from_millis(30))
            .await,
        3
    );
}

#[tokio::test]
async fn fixed_identity_sets_the_caller_once() {
    use a2a_protocol_server::{CallContext, ServerInterceptor as _};
    let ctx = CallContext::new("SendMessage");
    crate::support::FixedIdentity("run-1".to_owned())
        .before(&ctx)
        .await
        .expect("before succeeds");
    assert_eq!(ctx.caller_identity(), Some("run-1"));
}

#[test]
fn text_of_joins_only_text_parts() {
    use a2a_protocol_types::message::Part;
    let parts = vec![Part::text("a"), Part::text("b")];
    assert_eq!(crate::support::text_of(&parts), "ab");
    assert_eq!(crate::support::text_of(&[]), "");
}
