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

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use opentelemetry::logs::Severity;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{LogBatch, LogExporter};
use opentelemetry_sdk::metrics::Temporality;
use opentelemetry_sdk::metrics::data::ResourceMetrics;
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::trace::{SpanData, SpanExporter};

/// An exporter that adds one to `lost` for each export that fails after
/// whatever retries `inner` makes. Every other method is forwarded, so the
/// wrapper changes nothing else about the exporter.
#[derive(Debug)]
pub(super) struct Counting<E> {
    inner: E,
    lost: Arc<AtomicU64>,
}

impl<E> Counting<E> {
    pub(super) const fn new(inner: E, lost: Arc<AtomicU64>) -> Self {
        Self { inner, lost }
    }

    fn count(&self, result: OTelSdkResult) -> OTelSdkResult {
        if result.is_err() {
            self.lost.fetch_add(1, Ordering::Relaxed);
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
        async fn export(&self, _: Vec<SpanData>) -> OTelSdkResult {
            export_result(self)
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
        async fn export(&self, _: &ResourceMetrics) -> OTelSdkResult {
            export_result(self)
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
        async fn export(&self, _: LogBatch<'_>) -> OTelSdkResult {
            export_result(self)
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
}
