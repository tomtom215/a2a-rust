// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The telemetry pipeline's own health as a metric (audit C-13).
//!
//! A lost export used to be visible only as a rate-limited warning and in
//! what `shutdown` and `force_flush` return. When metrics are exported,
//! `a2a.telemetry.export.failed` counts, per signal, the exports that failed
//! after the exporter's retries and whose batch was dropped (`counting.rs`
//! decides what counts), so an alert can watch it.
//!
//! It is a name of this crate, not the semantic conventions'
//! `otel.sdk.exporter.*.exported`: those count items (spans, log records,
//! data points) with an `error.type`, and the wrapper sees whole batches. A
//! failing metrics exporter loses its own count with the batch, so this
//! is the more useful under a pull exporter or a second metrics exporter;
//! the warnings remain either way.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_sdk::metrics::SdkMeterProvider;

use super::SCOPE;
use super::config::Signal;

/// The counter's name.
pub(super) const FAILED_EXPORTS: &str = "a2a.telemetry.export.failed";

/// Registers the observable counter on `provider`, reading `totals`, the
/// cumulative count of failed exports for each exporting signal.
pub(super) fn observe_failed_exports(
    provider: &SdkMeterProvider,
    totals: Vec<(Signal, Arc<AtomicU64>)>,
) {
    if totals.is_empty() {
        return;
    }
    // The callback stays registered for the provider's life; the handle
    // returned need not be kept (opentelemetry_sdk 0.33.1).
    let _counter = provider
        .meter(SCOPE)
        .u64_observable_counter(FAILED_EXPORTS)
        .with_unit("{batch}")
        .with_description(
            "Exports that failed after the exporter's retries; their batch was dropped",
        )
        .with_callback(move |observer| {
            for (signal, total) in &totals {
                observer.observe(
                    total.load(Ordering::Relaxed),
                    &[KeyValue::new("signal", signal.name())],
                );
            }
        })
        .build();
}

#[cfg(test)]
mod tests {
    use opentelemetry_sdk::metrics::ManualReader;
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};

    use super::*;

    #[test]
    fn each_signals_failed_exports_are_observed_cumulatively() {
        let reader = Arc::new(ManualReader::builder().build());
        let provider = SdkMeterProvider::builder()
            .with_reader(crate::otel::test_reader::SharedReader(Arc::clone(&reader)))
            .build();
        let traces = Arc::new(AtomicU64::new(0));
        let logs = Arc::new(AtomicU64::new(0));
        observe_failed_exports(
            &provider,
            vec![
                (Signal::Traces, Arc::clone(&traces)),
                (Signal::Logs, Arc::clone(&logs)),
            ],
        );
        traces.store(3, Ordering::Relaxed);
        let read = |reader: &Arc<ManualReader>| {
            let exported = crate::otel::test_reader::collect(reader);
            let metric = exported
                .scope_metrics()
                .filter(|s| s.scope().name() == SCOPE)
                .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
                .find(|m| m.name() == FAILED_EXPORTS)
                .expect("the counter");
            assert_eq!(metric.unit(), "{batch}");
            let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
                panic!("a u64 sum");
            };
            assert!(sum.is_monotonic());
            let mut points: Vec<(String, u64)> = sum
                .data_points()
                .map(|p| {
                    let signal = p
                        .attributes()
                        .find(|kv| kv.key.as_str() == "signal")
                        .map(|kv| kv.value.to_string())
                        .unwrap_or_default();
                    (signal, p.value())
                })
                .collect();
            points.sort();
            points
        };
        assert_eq!(
            read(&reader),
            [("logs".to_owned(), 0), ("traces".to_owned(), 3)]
        );
        traces.fetch_add(1, Ordering::Relaxed);
        logs.fetch_add(2, Ordering::Relaxed);
        assert_eq!(
            read(&reader),
            [("logs".to_owned(), 2), ("traces".to_owned(), 4)]
        );
    }

    #[test]
    fn nothing_is_registered_without_an_exporting_signal() {
        let reader = Arc::new(ManualReader::builder().build());
        let provider = SdkMeterProvider::builder()
            .with_reader(crate::otel::test_reader::SharedReader(Arc::clone(&reader)))
            .build();
        observe_failed_exports(&provider, Vec::new());
        let exported = crate::otel::test_reader::collect(&reader);
        assert!(
            exported
                .scope_metrics()
                .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
                .all(|m| m.name() != FAILED_EXPORTS)
        );
    }
}
