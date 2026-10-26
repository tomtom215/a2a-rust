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
use super::hints::{self, Cause, Route};

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
    /// The proxy the export goes through, when one applies.
    via: Option<String>,
    route: Route,
    /// The transport's own words for the last failure, when the HTTP client
    /// kept them (`hints.rs`); gRPC's error already carries its own.
    cause: Arc<Cause>,
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
        let error = error.to_string();
        let cause = self.cause.take();
        let hint = hints::hint(&self.route, &error, cause.as_deref());
        let via = self
            .via
            .as_ref()
            .map_or_else(String::new, |proxy| format!(" via proxy {proxy}"));
        let cause = cause.map_or_else(String::new, |c| format!(" (cause: {c})"));
        let hint = hint.map_or_else(String::new, |h| format!(". Hint: {h}"));
        tracing::warn!(
            "OTLP export of {} to {}{via} failed after the exporter's retries and the batch \
             was dropped{more}: {error}{cause}{hint}",
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
    /// was going, with the transport's cause and a hint where one is certain.
    pub(super) fn reporting(
        mut self,
        signal: Signal,
        destination: &str,
        route: Route,
        via: Option<String>,
        cause: Arc<Cause>,
    ) -> Self {
        self.notice = Some(Notice {
            signal,
            destination: destination.to_owned(),
            via,
            route,
            cause,
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
mod tests;
