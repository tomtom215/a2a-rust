// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use super::super::config::OtlpProtocol;
use super::*;
use opentelemetry_sdk::error::OTelSdkError;

/// An exporter whose answers the test chooses. Each method other than
/// `export` fails with its own name, so a test can tell that the
/// wrapper's answer came from this method and not from a default.
#[derive(Debug, Default)]
struct Fake {
    fail_export: bool,
    enabled: bool,
    resource_set: bool,
}

fn failed(what: &str) -> OTelSdkResult {
    Err(OTelSdkError::InternalFailure(what.to_owned()))
}

fn export_result(fake: &Fake) -> OTelSdkResult {
    if fake.fail_export {
        failed("export")
    } else {
        Ok(())
    }
}

/// The name the inner exporter failed with; panics on anything else.
fn failure(result: OTelSdkResult) -> String {
    match result {
        Err(OTelSdkError::InternalFailure(what)) => what,
        other => panic!("expected the inner exporter's failure, got {other:?}"),
    }
}

impl SpanExporter for Fake {
    fn export(&self, _: Vec<SpanData>) -> impl Future<Output = OTelSdkResult> + Send {
        std::future::ready(export_result(self))
    }
    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        failed("shutdown_with_timeout")
    }
    fn shutdown(&self) -> OTelSdkResult {
        failed("shutdown")
    }
    fn force_flush(&self) -> OTelSdkResult {
        failed("force_flush")
    }
    fn set_resource(&mut self, _: &Resource) {
        self.resource_set = true;
    }
}

impl PushMetricExporter for Fake {
    fn export(&self, _: &ResourceMetrics) -> impl Future<Output = OTelSdkResult> + Send {
        std::future::ready(export_result(self))
    }
    fn force_flush(&self) -> OTelSdkResult {
        failed("force_flush")
    }
    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        failed("shutdown_with_timeout")
    }
    fn shutdown(&self) -> OTelSdkResult {
        failed("shutdown")
    }
    fn temporality(&self) -> Temporality {
        // Not the default (`Cumulative`), so a defaulted answer shows.
        Temporality::Delta
    }
}

impl LogExporter for Fake {
    fn export(&self, _: LogBatch<'_>) -> impl Future<Output = OTelSdkResult> + Send {
        std::future::ready(export_result(self))
    }
    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        failed("shutdown_with_timeout")
    }
    fn shutdown(&self) -> OTelSdkResult {
        failed("shutdown")
    }
    fn event_enabled(&self, _: Severity, _: &str, _: Option<&str>) -> bool {
        self.enabled
    }
    fn set_resource(&mut self, _: &Resource) {
        self.resource_set = true;
    }
}

fn counting(fail_export: bool) -> (Counting<Fake>, Arc<AtomicU64>) {
    let lost = Arc::new(AtomicU64::new(0));
    let fake = Fake {
        fail_export,
        ..Fake::default()
    };
    (Counting::new(fake, Arc::clone(&lost)), lost)
}

/// A failed export is counted once and still returned; a successful one
/// is not counted. On each of the three signals.
#[tokio::test]
async fn only_a_failed_export_is_counted() {
    for fail in [false, true] {
        let (c, lost) = counting(fail);
        let results = [
            SpanExporter::export(&c, Vec::new()).await,
            PushMetricExporter::export(&c, &ResourceMetrics::default()).await,
            LogExporter::export(&c, LogBatch::new(&[])).await,
        ];
        for result in results {
            assert_eq!(result.is_err(), fail, "{result:?}");
        }
        assert_eq!(lost.load(Ordering::Relaxed), if fail { 3 } else { 0 });
    }
}

/// Every other method answers with the inner exporter's answer.
#[test]
fn every_other_method_is_forwarded() {
    let (mut c, lost) = counting(false);
    let t = Duration::from_secs(1);
    assert_eq!(failure(SpanExporter::shutdown(&c)), "shutdown");
    assert_eq!(
        failure(SpanExporter::shutdown_with_timeout(&c, t)),
        "shutdown_with_timeout"
    );
    assert_eq!(failure(SpanExporter::force_flush(&c)), "force_flush");
    assert_eq!(failure(PushMetricExporter::shutdown(&c)), "shutdown");
    assert_eq!(
        failure(PushMetricExporter::shutdown_with_timeout(&c, t)),
        "shutdown_with_timeout"
    );
    assert_eq!(failure(PushMetricExporter::force_flush(&c)), "force_flush");
    assert_eq!(PushMetricExporter::temporality(&c), Temporality::Delta);
    assert_eq!(failure(LogExporter::shutdown(&c)), "shutdown");
    assert_eq!(
        failure(LogExporter::shutdown_with_timeout(&c, t)),
        "shutdown_with_timeout"
    );
    for enabled in [false, true] {
        c.inner.enabled = enabled;
        assert_eq!(
            LogExporter::event_enabled(&c, Severity::Info, "t", None),
            enabled
        );
    }
    // None of those is an export, so none is counted.
    assert_eq!(lost.load(Ordering::Relaxed), 0);

    let resource = Resource::builder_empty().build();
    SpanExporter::set_resource(&mut c, &resource);
    assert!(c.inner.resource_set, "spans");
    c.inner.resource_set = false;
    LogExporter::set_resource(&mut c, &resource);
    assert!(c.inner.resource_set, "logs");
}

// ── Saying so ────────────────────────────────────────────────────────

use tracing_subscriber::layer::SubscriberExt as _;

/// A `tracing` layer that keeps each event's target and message.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<(String, String)>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Events {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct MessageText(String);
        impl tracing::field::Visit for MessageText {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0 = format!("{v:?}");
                }
            }
        }
        let mut message = MessageText(String::new());
        event.record(&mut message);
        self.0
            .lock()
            .unwrap()
            .push((event.metadata().target().to_owned(), message.0));
    }
}

/// Records through `events` on this thread, until the guard drops.
fn recording(events: &Events) -> tracing::subscriber::DefaultGuard {
    tracing::subscriber::set_default(tracing_subscriber::registry().with(events.clone()))
}

/// A lost export is said once, naming the signal and where it was going,
/// on a target that is never exported — otherwise reporting the loss of an
/// export would itself produce something to export.
#[tokio::test]
async fn a_lost_export_is_reported_on_a_target_that_is_never_exported() {
    let events = Events::default();
    let _guard = recording(&events);
    let (c, _) = counting(true);
    let c = c.reporting(
        Signal::Traces,
        "http://collector:4318/v1/traces",
        route(
            OtlpProtocol::HttpProtobuf,
            "http://collector:4318/v1/traces",
        ),
        None,
        Arc::default(),
    );
    assert!(SpanExporter::export(&c, Vec::new()).await.is_err());
    let seen = events.0.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    let (target, message) = &seen[0];
    assert!(super::super::is_self_target(target), "{target}");
    assert!(
        message.contains("traces")
            && message.contains("http://collector:4318/v1/traces")
            && message.contains("dropped"),
        "{message}"
    );
}

#[tokio::test]
async fn a_successful_export_and_an_unreporting_wrapper_say_nothing() {
    let events = Events::default();
    let _guard = recording(&events);
    let (ok, _) = counting(false);
    let ok = ok.reporting(
        Signal::Logs,
        "http://c:1",
        route(OtlpProtocol::HttpProtobuf, "http://c:1"),
        None,
        Arc::default(),
    );
    assert!(LogExporter::export(&ok, LogBatch::new(&[])).await.is_ok());
    let (silent, lost) = counting(true);
    assert!(SpanExporter::export(&silent, Vec::new()).await.is_err());
    assert_eq!(lost.load(Ordering::Relaxed), 1);
    assert!(events.0.lock().unwrap().is_empty());
}

/// While exports keep failing the report repeats only every
/// [`REPORT_EVERY`], and says how many it held back.
#[test]
fn reports_are_rate_limited_and_count_what_they_held_back() {
    let notice = Notice {
        signal: Signal::Metrics,
        destination: String::new(),
        via: None,
        route: route(OtlpProtocol::HttpProtobuf, ""),
        cause: Arc::default(),
        last: Mutex::new(None),
        held_back: AtomicU64::new(0),
    };
    let t0 = Instant::now();
    let s = Duration::from_secs;
    assert_eq!(notice.permit(t0), Some(0));
    assert_eq!(notice.permit(t0 + s(1)), None);
    assert_eq!(notice.permit(t0 + s(29)), None);
    assert_eq!(notice.permit(t0 + s(30)), Some(2));
    assert_eq!(notice.permit(t0 + s(31)), None);
    assert_eq!(notice.permit(t0 + s(61)), Some(1));
}

/// A report that follows held-back ones says how many; the first one, and any
/// other with none held back, does not.
#[test]
fn the_report_counts_what_was_held_back_only_when_some_was() {
    let events = Events::default();
    let _guard = recording(&events);
    let notice = |held_back: u64| Notice {
        signal: Signal::Metrics,
        destination: "http://c:4318".to_owned(),
        via: None,
        route: route(OtlpProtocol::HttpProtobuf, "http://c:4318"),
        cause: Arc::default(),
        last: Mutex::new(None),
        held_back: AtomicU64::new(held_back),
    };
    let error = OTelSdkError::InternalFailure("export".to_owned());
    notice(0).lost(&error);
    notice(3).lost(&error);
    let seen = events.0.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(
        !seen[0].1.contains("more since the last report"),
        "{}",
        seen[0].1
    );
    assert!(
        seen[1].1.contains("(3 more since the last report)"),
        "{}",
        seen[1].1
    );
}

fn route(protocol: OtlpProtocol, url: &str) -> Route {
    Route::of(protocol, url)
}

/// The first warning `export` produced, for a lost export to `url` with
/// `cause` kept by the HTTP client and the export going `via` a proxy.
async fn report(
    protocol: OtlpProtocol,
    url: &str,
    via: Option<&str>,
    cause: Option<&str>,
) -> String {
    let events = Events::default();
    let _guard = recording(&events);
    let slot = Arc::new(Cause::default());
    if let Some(cause) = cause {
        slot.record_text(cause.to_owned());
    }
    let (c, _) = counting(true);
    let c = c.reporting(
        Signal::Traces,
        url,
        route(protocol, url),
        via.map(str::to_owned),
        slot,
    );
    assert!(SpanExporter::export(&c, Vec::new()).await.is_err());
    let seen = events.0.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    seen[0].1.clone()
}

/// The transport's own words are in the report, because the exporter
/// crate's say only "network error"; and a certificate that no trusted CA
/// issued is told apart from a collector that is down.
#[tokio::test]
async fn the_report_carries_the_cause_and_a_hint_that_the_cause_licenses() {
    let message = report(
        OtlpProtocol::HttpProtobuf,
        "https://c:4318/v1/traces",
        None,
        Some("client error (Connect): invalid peer certificate: UnknownIssuer"),
    )
    .await;
    assert!(
        message
            .contains("(cause: client error (Connect): invalid peer certificate: UnknownIssuer)"),
        "{message}"
    );
    assert!(
        message.contains("OTEL_EXPORTER_OTLP_CERTIFICATE"),
        "{message}"
    );

    // A refused connection has a cause and no advice: nothing in it says
    // what to change.
    let refused = report(
        OtlpProtocol::HttpProtobuf,
        "http://c:4318/v1/traces",
        None,
        Some("client error (Connect): tcp connect error: Connection refused (os error 111)"),
    )
    .await;
    assert!(refused.contains("Connection refused"), "{refused}");
    assert!(!refused.contains("OTEL_EXPORTER_OTLP"), "{refused}");
}

#[tokio::test]
async fn a_proxy_the_export_goes_through_is_named() {
    let message = report(
        OtlpProtocol::HttpProtobuf,
        "http://c:4318/v1/traces",
        Some("http://proxy.example:3128"),
        None,
    )
    .await;
    assert!(
        message.contains("to http://c:4318/v1/traces via proxy http://proxy.example:3128 failed"),
        "{message}"
    );
}

#[tokio::test]
async fn the_other_transports_port_is_named_when_the_export_fails() {
    let message = report(
        OtlpProtocol::HttpProtobuf,
        "http://c:4317/v1/traces",
        None,
        None,
    )
    .await;
    assert!(
        message.contains("OTEL_EXPORTER_OTLP_PROTOCOL=grpc"),
        "{message}"
    );
}
