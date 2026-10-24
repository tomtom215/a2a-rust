// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`GlobalOtelMetrics`]: the handler's default metrics, on the global meter
//! provider as it stands at each measurement.
//!
//! `OtelMetrics` binds its instruments to one meter when it is built. As the
//! default, built when `RequestHandlerBuilder::new` ran, that bound them to
//! whatever provider was global then: a process that built its handler
//! before installing `Telemetry` — or installed none, then one — recorded
//! into OpenTelemetry's no-op meter for its whole life, with nothing to say
//! so. This asks the global for its provider on each measurement, keeps the
//! instruments while it is the same provider, and builds them again from a
//! new one when it changes.
//!
//! The cost is a read lock on the global and a pointer comparison per
//! measurement; instruments are built once per provider. The old provider's
//! handle is kept until replaced, so its address cannot be reused by the new
//! one and mistaken for it.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use opentelemetry::metrics::MeterProvider;

use super::OtelMetrics;
use crate::metrics::{ConnectionPoolStats, Metrics, RpcCall};

/// The instrumentation scope, as `OtelMetricsBuilder` defaults to it.
const METER_NAME: &str = "a2a.server";

/// [`OtelMetrics`] on the global meter provider, rebound when it changes.
pub struct GlobalOtelMetrics {
    bound: RwLock<Option<Bound>>,
    /// Where the provider comes from: the global, except in this module's
    /// tests, which must not race the crate's other tests for it.
    source: fn() -> Arc<dyn MeterProvider + Send + Sync>,
}

/// The provider the instruments were built from, and the instruments.
struct Bound {
    provider: Arc<dyn MeterProvider + Send + Sync>,
    metrics: Arc<OtelMetrics>,
}

impl std::fmt::Debug for GlobalOtelMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalOtelMetrics").finish_non_exhaustive()
    }
}

impl GlobalOtelMetrics {
    /// Bound to nothing yet: the first measurement binds.
    pub fn new() -> Self {
        Self {
            bound: RwLock::new(None),
            source: opentelemetry::global::meter_provider,
        }
    }

    /// The instruments for the provider that is global now.
    fn current(&self) -> Arc<OtelMetrics> {
        let provider = (self.source)();
        if let Ok(bound) = self.bound.read()
            && let Some(bound) = bound.as_ref()
            && Arc::ptr_eq(&bound.provider, &provider)
        {
            return Arc::clone(&bound.metrics);
        }
        let metrics = Arc::new(OtelMetrics::from_meter(&provider.meter(METER_NAME)));
        if let Ok(mut bound) = self.bound.write() {
            *bound = Some(Bound {
                provider,
                metrics: Arc::clone(&metrics),
            });
        }
        metrics
    }
}

impl Metrics for GlobalOtelMetrics {
    fn on_request(&self, method: &str) {
        self.current().on_request(method);
    }

    fn on_response(&self, method: &str) {
        self.current().on_response(method);
    }

    fn on_error(&self, method: &str, error: &str) {
        self.current().on_error(method, error);
    }

    fn on_latency(&self, method: &str, duration: Duration) {
        self.current().on_latency(method, duration);
    }

    fn on_queue_depth_change(&self, active_queues: usize) {
        self.current().on_queue_depth_change(active_queues);
    }

    fn on_persistence_error(&self, operation: &str, error_kind: &str) {
        self.current().on_persistence_error(operation, error_kind);
    }

    fn on_push_delivery(&self, outcome: &str) {
        self.current().on_push_delivery(outcome);
    }

    fn on_connection_pool_stats(&self, stats: &ConnectionPoolStats) {
        self.current().on_connection_pool_stats(stats);
    }

    fn on_rpc_call(&self, call: &RpcCall<'_>) {
        self.current().on_rpc_call(call);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, LazyLock};

    use opentelemetry::metrics::MeterProvider;
    use opentelemetry_sdk::metrics::data::ResourceMetrics;
    use opentelemetry_sdk::metrics::reader::MetricReader as _;
    use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

    use super::GlobalOtelMetrics;
    use crate::metrics::{ConnectionPoolStats, Metrics};
    use crate::otel::test_reader::SharedReader;

    static READER: LazyLock<Arc<ManualReader>> =
        LazyLock::new(|| Arc::new(ManualReader::builder().build()));
    static PROVIDER: LazyLock<Arc<dyn MeterProvider + Send + Sync>> = LazyLock::new(|| {
        Arc::new(
            SdkMeterProvider::builder()
                .with_reader(SharedReader(Arc::clone(&READER)))
                .build(),
        )
    });

    fn provider() -> Arc<dyn MeterProvider + Send + Sync> {
        Arc::clone(&PROVIDER)
    }

    /// The debug form names the type and nothing it holds (the mutation gate
    /// found it untested).
    #[test]
    fn debug_names_the_type_only() {
        assert_eq!(
            format!("{:?}", GlobalOtelMetrics::new()),
            "GlobalOtelMetrics { .. }"
        );
    }

    /// Every callback reaches the provider's instruments: each one, emptied,
    /// leaves its instrument unrecorded (the mutation gate found five that
    /// could be).
    #[test]
    fn every_callback_records_on_the_provider() {
        let metrics = GlobalOtelMetrics {
            source: provider,
            ..GlobalOtelMetrics::new()
        };
        metrics.on_request("GetTask");
        metrics.on_response("GetTask");
        metrics.on_error("GetTask", "task_not_found");
        metrics.on_latency("GetTask", std::time::Duration::from_millis(3));
        metrics.on_queue_depth_change(2);
        metrics.on_persistence_error("save", "internal");
        metrics.on_push_delivery("delivered");
        metrics.on_connection_pool_stats(&ConnectionPoolStats::default());
        let mut collected = ResourceMetrics::default();
        READER.collect(&mut collected).expect("collect");
        let mut names: Vec<String> = collected
            .scope_metrics()
            .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
            .map(|m| m.name().to_owned())
            .collect();
        names.sort();
        for name in [
            "a2a.server.requests",
            "a2a.server.responses",
            "a2a.server.errors",
            "a2a.server.request.duration",
            "a2a.server.queue_depth",
            "a2a.server.persistence_errors",
            "a2a.server.push_deliveries",
            "a2a.server.pool.active",
        ] {
            assert!(names.iter().any(|n| n == name), "{name} in {names:?}");
        }
    }
}
