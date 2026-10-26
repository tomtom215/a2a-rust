// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Counting each signal's lost exports, on either transport.
//!
//! The SDK's batch processors log a failed export and drop the batch, and
//! their `shutdown` returns `Ok` regardless (`opentelemetry_sdk` 0.33.1,
//! `trace/span_processor.rs`: the caller maps the worker's reply to `Ok`
//! without reading it). `Telemetry` wraps every exporter in [`Counting`] so
//! its own `shutdown` and `force_flush` can say that telemetry was lost.
//!
//! The count is taken from what `export` returns, not from HTTP responses.
//! Since 0.33 `opentelemetry-otlp` retries a failed export (four attempts by
//! default), so a refused attempt is not a lost export, and only the exporter
//! knows when it has given up. Counted per response, an export refused once
//! and then accepted would be reported lost. tonic's client offers no hook
//! at all, so per-response counting also left gRPC unreported.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use opentelemetry::logs::Severity;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};
use opentelemetry_sdk::logs::{LogBatch, LogExporter};
use opentelemetry_sdk::metrics::Temporality;
use opentelemetry_sdk::metrics::data::ResourceMetrics;
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::trace::{SpanData, SpanExporter};

use super::config::Signal;

/// How often a lost export is reported while it keeps being lost: a collector
/// that is down loses a batch every few seconds, and a line for each would
/// drown the log it is meant to be found in.
const REPORT_EVERY: Duration = Duration::from_secs(30);

/// Says, at most once per [`REPORT_EVERY`], that an export was lost — on the
/// `tracing` target this module's diagnostics use, which is never exported
/// (`SELF_TARGETS`) and is shown by `Telemetry::install_subscriber`.
///
/// The lost batches were silent before: the SDK logs them through `tracing`
/// under its own target, which no subscriber of a process that only called
/// `install_subscriber` showed. Measured 2026-10-26: a collector that was
/// not there, nine seconds, no output at all.
#[derive(Debug)]
struct Notice {
    signal: Signal,
    destination: String,
    last: Mutex<Option<Instant>>,
    held_back: AtomicU64,
}

impl Notice {
    /// Whether a report may be made at `now`, and how many were held back
    /// since the last one.
    fn permit(&self, now: Instant) -> Option<u64> {
        let too_soon = {
            let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
            let too_soon = last.is_some_and(|t| now.saturating_duration_since(t) < REPORT_EVERY);
            if !too_soon {
                *last = Some(now);
            }
            too_soon
        };
        if too_soon {
            self.held_back.fetch_add(1, Ordering::Relaxed);
            None
        } else {
            Some(self.held_back.swap(0, Ordering::Relaxed))
        }
    }

    fn lost(&self, error: &OTelSdkError) {
        let Some(held_back) = self.permit(Instant::now()) else {
            return;
        };
        let more = if held_back == 0 {
            String::new()
        } else {
            format!(" ({held_back} more since the last report)")
        };
        tracing::warn!(
            "OTLP export of {} to {} failed after the exporter's retries and the batch \
             was dropped{more}: {error}",
            self.signal.name(),
            self.destination
        );
    }
}

/// An exporter that adds one to `lost` for each export that fails after
/// whatever retries `inner` makes. Every other method is forwarded, so the
/// wrapper changes nothing else about the exporter.
#[derive(Debug)]
pub(super) struct Counting<E> {
    inner: E,
    lost: Arc<AtomicU64>,
    notice: Option<Notice>,
}

impl<E> Counting<E> {
    pub(super) const fn new(inner: E, lost: Arc<AtomicU64>) -> Self {
        Self {
            inner,
            lost,
            notice: None,
        }
    }

    /// Also report a lost export, rate-limited, naming `signal` and where it
    /// was going.
    pub(super) fn reporting(mut self, signal: Signal, destination: &str) -> Self {
        self.notice = Some(Notice {
            signal,
            destination: destination.to_owned(),
            last: Mutex::new(None),
            held_back: AtomicU64::new(0),
        });
        self
    }

    fn count(&self, result: OTelSdkResult) -> OTelSdkResult {
        if let Err(error) = &result {
            self.lost.fetch_add(1, Ordering::Relaxed);
            if let Some(notice) = &self.notice {
                notice.lost(error);
            }
        }
        result
    }
}

impl<E: SpanExporter> SpanExporter for Counting<E> {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        self.count(self.inner.export(batch).await)
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn shutdown(&self) -> OTelSdkResult {
        self.inner.shutdown()
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.inner.force_flush()
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

impl<E: PushMetricExporter> PushMetricExporter for Counting<E> {
    async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
        self.count(self.inner.export(metrics).await)
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.inner.force_flush()
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn shutdown(&self) -> OTelSdkResult {
        self.inner.shutdown()
    }

    fn temporality(&self) -> Temporality {
        self.inner.temporality()
    }
}

impl<E: LogExporter> LogExporter for Counting<E> {
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        self.count(self.inner.export(batch).await)
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn shutdown(&self) -> OTelSdkResult {
        self.inner.shutdown()
    }

    fn event_enabled(&self, level: Severity, target: &str, name: Option<&str>) -> bool {
        self.inner.event_enabled(level, target, name)
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

#[cfg(test)]
mod tests {
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
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            struct Message(String);
            impl tracing::field::Visit for Message {
                fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                    if f.name() == "message" {
                        self.0 = format!("{v:?}");
                    }
                }
            }
            let mut message = Message(String::new());
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
        let c = c.reporting(Signal::Traces, "http://collector:4318/v1/traces");
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
        let ok = ok.reporting(Signal::Logs, "http://c:1");
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
}
