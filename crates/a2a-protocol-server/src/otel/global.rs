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
    pub const fn new() -> Self {
        Self {
            bound: RwLock::new(None),
        }
    }

    /// The instruments for the provider that is global now.
    fn current(&self) -> Arc<OtelMetrics> {
        let provider = opentelemetry::global::meter_provider();
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
