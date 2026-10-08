// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `Telemetry` end to end: what it exports, over each transport, decoded by
//! a stand-in collector.
//!
//! Every builder here sets `install_globally(false)` and records through a
//! thread-local subscriber (`tracing::subscriber::with_default`), so the
//! tests share no global state and can run in parallel. Each sets its
//! endpoint and protocol in code, which wins over the process environment,
//! so a developer's own `OTEL_EXPORTER_OTLP_*` cannot redirect them.

#![cfg(feature = "otel")]

mod agent_spans;
mod collector;

use std::time::{Duration, Instant};

use a2a_protocol_server::otel::{Langfuse, OtlpProtocol, Telemetry, TelemetryBuilder};
use collector::{Collector, attr, test_certs};
use opentelemetry::metrics::MeterProvider as _;
use tracing_subscriber::layer::SubscriberExt as _;

fn builder(base: &str, protocol: OtlpProtocol) -> TelemetryBuilder {
    Telemetry::builder()
        .install_globally(false)
        .with_otlp_endpoint(base)
        .with_otlp_protocol(protocol)
        .with_default_service_name("telemetry-test")
}

/// Records one span carrying an attribute, one `INFO` event inside it, one
/// `DEBUG` event, and one counter increment.
fn record_one_of_each(telemetry: &Telemetry) {
    let subscriber = tracing_subscriber::registry().with(telemetry.layer());
    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("unit-of-work", answer = 42);
        let _entered = span.enter();
        tracing::info!("hello from the test");
        tracing::debug!("debug detail");
    });
    if let Some(meter) = telemetry.meter_provider() {
        meter
            .meter("telemetry-test")
            .u64_counter("probe.count")
            .build()
            .add(1, &[]);
    }
}

fn assert_all_three_arrived(collector: &Collector) {
    let spans = collector.spans();
    let (resource, span) = spans
        .iter()
        .find(|(_, s)| s.name == "unit-of-work")
        .unwrap_or_else(|| panic!("no `unit-of-work` span among {:?}", collector.paths()));
    assert_eq!(
        attr(resource, "service.name").as_deref(),
        Some("telemetry-test")
    );
    assert_eq!(attr(&span.attributes, "answer").as_deref(), Some("42"));
    assert!(
        collector
            .log_bodies()
            .iter()
            .any(|b| b == "hello from the test"),
        "log bodies: {:?}",
        collector.log_bodies()
    );
    assert!(
        collector.metric_names().iter().any(|m| m == "probe.count"),
        "metrics: {:?}",
        collector.metric_names()
    );
}

/// OTLP/HTTP, from a plain `#[test]` with no Tokio runtime anywhere — the
/// case `init_otlp_pipeline` panics on.
#[test]
fn http_exports_traces_metrics_and_logs_without_any_runtime() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
    let mut paths = collector.paths();
    paths.sort();
    paths.dedup();
    assert_eq!(paths, ["/v1/logs", "/v1/metrics", "/v1/traces"]);
}

/// OTLP/gRPC, with a header carried as gRPC metadata.
#[test]
fn grpc_exports_traces_metrics_and_logs_with_headers() {
    let collector = Collector::grpc();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::Grpc)
        .with_otlp_header("x-tenant", "acme")
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
    for r in collector.received() {
        assert_eq!(
            r.headers.get("x-tenant").map(String::as_str),
            Some("acme"),
            "{}",
            r.path
        );
    }
}

/// Shutting down from inside a `current_thread` runtime — the application's
/// only thread blocked on the flush — completes, and the flush lands.
#[tokio::test(flavor = "current_thread")]
async fn shutdown_inside_a_current_thread_runtime_neither_deadlocks_nor_loses_data() {
    for protocol in [OtlpProtocol::HttpProtobuf, OtlpProtocol::Grpc] {
        let collector = if protocol == OtlpProtocol::Grpc {
            Collector::grpc()
        } else {
            Collector::http()
        };
        let telemetry = builder(&collector.http_base(), protocol)
            .build()
            .expect("build");
        record_one_of_each(&telemetry);
        let started = Instant::now();
        telemetry.shutdown().expect("every signal flushed");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{protocol:?}: shutdown took {:?}, which is a timeout, not a flush",
            started.elapsed()
        );
        assert_all_three_arrived(&collector);
    }
}

/// Dropping the guard flushes, as `shutdown` does.
#[test]
fn dropping_the_guard_flushes() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    drop(telemetry);
    assert_all_three_arrived(&collector);
}

/// A collector behind a private CA is reached once the CA is given …
#[test]
fn https_with_the_private_ca_exports() {
    let certs = test_certs();
    let collector = Collector::https(&certs);
    let telemetry = builder(&collector.https_base(), OtlpProtocol::HttpProtobuf)
        .with_ca_certificate_pem(&certs.ca_pem)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
}

/// … and is not reached without it: the handshake fails and nothing is
/// recorded. Without this the test above could pass with verification off.
#[test]
fn https_without_the_private_ca_exports_nothing() {
    let certs = test_certs();
    let collector = Collector::https(&certs);
    let telemetry = builder(&collector.https_base(), OtlpProtocol::HttpProtobuf)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    // The SDK's own shutdown says `Ok` here; the failures are counted in
    // this crate's HTTP client and reported, so the loss is not silent.
    let err = telemetry.shutdown().expect_err("every export failed");
    let msg = err.to_string();
    for signal in ["traces", "metrics", "logs"] {
        assert!(msg.contains(&format!("{signal}: ")), "{msg}");
    }
    assert!(collector.received().is_empty(), "{:?}", collector.paths());
}

/// A collector that cannot be reached is reported by `shutdown`, though the
/// SDK's own shutdown says `Ok`. A collector answering `200` is covered by
/// every test above that expects `Ok`.
#[test]
fn an_unreachable_collector_is_reported_by_shutdown() {
    // A port bound and released: nothing listens on it.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr").port()
    };
    let telemetry = builder(
        &format!("http://127.0.0.1:{port}"),
        OtlpProtocol::HttpProtobuf,
    )
    .with_metrics(false)
    .with_logs(false)
    .build()
    .expect("build");
    record_one_of_each(&telemetry);
    let err = telemetry.shutdown().expect_err("the export failed");
    assert!(err.to_string().contains("traces: 1 export(s)"), "{err}");
}

/// The Langfuse preset sends traces only, to Langfuse's route, with its
/// headers — and no metrics or logs, which Langfuse would drop or refuse.
#[test]
fn langfuse_preset_sends_only_traces_with_basic_auth_and_ingestion_version() {
    let collector = Collector::http();
    let telemetry = Telemetry::builder()
        .install_globally(false)
        .with_langfuse(Langfuse::new(collector.http_base(), "pk-lf-t", "sk-lf-t"))
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("traces flushed");
    let received = collector.received();
    assert!(!received.is_empty());
    for r in &received {
        assert_eq!(r.path, "/api/public/otel/v1/traces");
        // base64("pk-lf-t:sk-lf-t") from `printf 'pk-lf-t:sk-lf-t' | base64`.
        assert_eq!(
            r.headers.get("authorization").map(String::as_str),
            Some("Basic cGstbGYtdDpzay1sZi10")
        );
        assert_eq!(
            r.headers
                .get("x-langfuse-ingestion-version")
                .map(String::as_str),
            Some("4")
        );
        assert_eq!(
            r.headers.get("content-type").map(String::as_str),
            Some("application/x-protobuf")
        );
    }
    assert!(
        collector
            .spans()
            .iter()
            .any(|(_, s)| s.name == "unit-of-work")
    );
}

/// The exporter's own stack is never exported, by span or by event; an
/// application target that merely contains one of those names is.
#[test]
fn the_exporters_own_targets_are_not_exported() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .build()
        .expect("build");
    let subscriber = tracing_subscriber::registry().with(telemetry.layer());
    tracing::subscriber::with_default(subscriber, || {
        let _h = tracing::info_span!(target: "hyper::client", "hyper-span").entered();
        tracing::info!(target: "h2::proto", "h2 event");
        drop(_h);
        let _a = tracing::info_span!(target: "my_app::hyper", "app-span").entered();
        tracing::info!(target: "my_app::hyper", "app event");
    });
    telemetry.shutdown().expect("flushed");
    let names: Vec<String> = collector.spans().into_iter().map(|(_, s)| s.name).collect();
    assert!(names.contains(&"app-span".to_owned()), "{names:?}");
    assert!(!names.contains(&"hyper-span".to_owned()), "{names:?}");
    let logs = collector.log_bodies();
    assert!(logs.contains(&"app event".to_owned()), "{logs:?}");
    assert!(!logs.contains(&"h2 event".to_owned()), "{logs:?}");
}

/// Events below the log level are not exported; lowering it exports them.
#[test]
fn the_log_level_decides_which_events_are_exported() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("flushed");
    assert!(!collector.log_bodies().contains(&"debug detail".to_owned()));

    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .with_log_level(tracing::Level::DEBUG)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("flushed");
    assert!(collector.log_bodies().contains(&"debug detail".to_owned()));
}

/// `otel_metrics()` records the server's catalogue through this telemetry's
/// own meter provider, without the global one.
#[test]
fn otel_metrics_records_through_this_telemetrys_meter() {
    use a2a_protocol_server::metrics::Metrics as _;
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .build()
        .expect("build");
    telemetry.otel_metrics().on_request("SendMessage");
    telemetry.shutdown().expect("flushed");
    assert!(
        collector
            .metric_names()
            .iter()
            .any(|m| m == "a2a.server.requests"),
        "{:?}",
        collector.metric_names()
    );
}

/// With one signal off, the other still records. The first cut composed an
/// absent signal as `None`, whose level hint is `OFF`, and every span was
/// silently dropped whenever logs were off — the Langfuse preset's default.
#[test]
fn each_signal_records_with_the_other_off() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .with_logs(false)
        .with_metrics(false)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("flushed");
    assert!(
        collector
            .spans()
            .iter()
            .any(|(_, s)| s.name == "unit-of-work")
    );

    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .with_traces(false)
        .with_metrics(false)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("flushed");
    assert!(
        collector
            .log_bodies()
            .contains(&"hello from the test".to_owned())
    );
}

/// With every signal off, the layer is inert: another layer on the same
/// subscriber still sees every event.
#[test]
fn a_layer_with_every_signal_off_does_not_silence_other_layers() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Count(Arc<AtomicUsize>);
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Count {
        fn on_event(&self, _: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let telemetry = Telemetry::builder()
        .install_globally(false)
        .with_traces(false)
        .with_metrics(false)
        .with_logs(false)
        .build()
        .expect("build");
    let seen = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry()
        .with(telemetry.layer())
        .with(Count(Arc::clone(&seen)));
    tracing::subscriber::with_default(subscriber, || tracing::info!("counted"));
    assert_eq!(seen.load(Ordering::SeqCst), 1);
}

/// A collector that answers an export with an error status refused it, and
/// that is reported the same way.
#[test]
fn a_collector_refusing_exports_is_reported_by_shutdown() {
    let collector = Collector::http_answering(503);
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .with_metrics(false)
        .with_logs(false)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    let err = telemetry.shutdown().expect_err("the export was refused");
    assert!(err.to_string().contains("traces: 1 export(s)"), "{err}");
    assert_eq!(
        collector.paths(),
        ["/v1/traces"],
        "it did reach the collector"
    );
}

/// gRPC to a collector behind a private CA, once the CA is given — the path
/// where `opentelemetry-otlp`'s own default enables no roots at all …
#[test]
fn grpc_over_tls_with_the_private_ca_exports() {
    let certs = test_certs();
    let collector = Collector::grpcs(&certs);
    let telemetry = builder(&collector.https_base(), OtlpProtocol::Grpc)
        .with_ca_certificate_pem(&certs.ca_pem)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
}

/// … and without it, the handshake fails and nothing arrives.
#[test]
fn grpc_over_tls_without_the_private_ca_exports_nothing() {
    let certs = test_certs();
    let collector = Collector::grpcs(&certs);
    let telemetry = builder(&collector.https_base(), OtlpProtocol::Grpc)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    let _ = telemetry.shutdown();
    assert!(collector.received().is_empty(), "{:?}", collector.paths());
}

/// `force_flush` reports a lost export as `shutdown` does.
#[test]
fn an_unreachable_collector_is_reported_by_force_flush() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr").port()
    };
    let telemetry = builder(
        &format!("http://127.0.0.1:{port}"),
        OtlpProtocol::HttpProtobuf,
    )
    .with_metrics(false)
    .with_logs(false)
    .build()
    .expect("build");
    record_one_of_each(&telemetry);
    let err = telemetry.force_flush().expect_err("the export failed");
    assert!(err.to_string().contains("traces: 1 export(s)"), "{err}");
}

/// Dropping the guard flushes and shuts down, even while something else
/// still holds a provider — which would otherwise keep it from flushing.
#[test]
fn dropping_the_guard_flushes_what_was_recorded() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .build()
        .expect("build");
    let held = telemetry.tracer_provider().cloned();
    record_one_of_each(&telemetry);
    drop(telemetry);
    assert_all_three_arrived(&collector);
    drop(held);
}
