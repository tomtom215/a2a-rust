// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `rpc.client.call.duration`: the client's half of the server's
//! `rpc.server.call.duration` (ADR 0013).
//!
//! The semantic conventions' `docs/rpc/rpc-metrics.md` (at `55c98cf`): a
//! histogram in `s` with the same advisory buckets as the server's, carrying
//! `rpc.system.name` (Required), `rpc.method`, `rpc.status_code`,
//! `error.type` "if and only if the operation failed", and `server.address`
//! and `server.port` (Conditionally Required on the client, if available).
//! Its value "SHOULD be the same as the RPC client span duration", so a
//! streaming call is measured until its stream is dropped, as its span is.
//!
//! It records through the global meter provider as it stands at each call —
//! the one `Telemetry` installs — under the meter `a2a.client`, building the
//! histogram again when the process installs another, so a client built
//! before `Telemetry` records once it is installed (until 0.15 it recorded
//! nothing all its life). A call on a custom transport that names no
//! `rpc.system.name` ([`Transport::rpc_system`](crate::transport::Transport::rpc_system))
//! records nothing, since that attribute is required.

use std::sync::{Arc, RwLock};
use std::time::Instant;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Histogram, MeterProvider};

use crate::call_span::{Peer, Status};

/// The conventions' advisory buckets for both call-duration histograms
/// (`docs/rpc/rpc-metrics.md` at `55c98cf`): 0.1 ms to 100 s, as the
/// server's `RPC_DURATION_BUCKETS`.
const BUCKETS: [f64; 13] = [
    0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 50.0, 100.0,
];

/// The provider a histogram was built from, and the histogram.
type Bound = (Arc<dyn MeterProvider + Send + Sync>, Histogram<f64>);

/// The client's `rpc.client.call.duration` histogram, on the global provider
/// as it is at each call. The provider it was last built from is kept, so
/// its address cannot be reused by another and mistaken for it.
#[derive(Clone)]
pub struct CallMetric {
    bound: Arc<RwLock<Option<Bound>>>,
    /// A histogram a test gave, recorded to instead of the global's.
    #[cfg(test)]
    fixed: Option<Histogram<f64>>,
}

impl std::fmt::Debug for CallMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallMetric").finish_non_exhaustive()
    }
}

/// The histogram, from `meter`.
fn histogram(meter: &opentelemetry::metrics::Meter) -> Histogram<f64> {
    meter
        .f64_histogram("rpc.client.call.duration")
        .with_description("Measures the duration of an outgoing Remote Procedure Call (RPC).")
        .with_unit("s")
        .with_boundaries(BUCKETS.to_vec())
        .build()
}

impl CallMetric {
    /// The histogram, from the global meter provider as it stands at each
    /// call.
    pub(crate) fn global() -> Self {
        Self {
            bound: Arc::new(RwLock::new(None)),
            #[cfg(test)]
            fixed: None,
        }
    }

    /// The histogram, from `meter`, for a test.
    #[cfg(test)]
    pub(crate) fn from_meter(meter: &opentelemetry::metrics::Meter) -> Self {
        Self {
            fixed: Some(histogram(meter)),
            ..Self::global()
        }
    }

    /// The histogram to record to now.
    fn current(&self) -> Histogram<f64> {
        #[cfg(test)]
        if let Some(h) = &self.fixed {
            return h.clone();
        }
        let bound = &self.bound;
        let provider = opentelemetry::global::meter_provider();
        if let Ok(slot) = bound.read()
            && let Some((built_from, h)) = slot.as_ref()
            && Arc::ptr_eq(built_from, &provider)
        {
            return h.clone();
        }
        let h = histogram(&provider.meter("a2a.client"));
        if let Ok(mut slot) = bound.write() {
            *slot = Some((provider, h.clone()));
        }
        h
    }

    /// Records one call of `method` — fully qualified, as `rpc.method` — to
    /// `peer`, begun at `started`, which ended with `status`.
    pub(crate) fn record(
        &self,
        peer: &Peer,
        method: &'static str,
        started: Instant,
        status: &Status,
    ) {
        let Some(system) = peer.system else {
            return;
        };
        let mut attributes = Vec::with_capacity(6);
        attributes.push(KeyValue::new("rpc.system.name", system));
        attributes.push(KeyValue::new("rpc.method", method));
        if let Some(code) = &status.code {
            attributes.push(KeyValue::new("rpc.status_code", code.clone()));
        }
        if let Some(error) = &status.error {
            attributes.push(KeyValue::new("error.type", error.clone()));
        }
        if let Some(address) = &peer.address {
            attributes.push(KeyValue::new("server.address", address.clone()));
            if let Some(port) = peer.port {
                attributes.push(KeyValue::new("server.port", i64::from(port)));
            }
        }
        self.current()
            .record(started.elapsed().as_secs_f64(), &attributes);
    }
}

#[cfg(test)]
mod tests;
