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
//! It records through the global meter provider as it stands when the client
//! is built — the one `Telemetry` installs — under the meter `a2a.client`. A
//! client built before a provider is installed records nothing, as an
//! `OtelMetrics` built then would; so does a call on a custom transport that
//! names no `rpc.system.name` ([`Transport::rpc_system`](crate::transport::Transport::rpc_system)),
//! since that attribute is required.

use std::time::Instant;

use opentelemetry::KeyValue;
use opentelemetry::metrics::Histogram;

use crate::call_span::{Peer, Status};

/// The conventions' advisory buckets for both call-duration histograms.
const BUCKETS: [f64; 14] = [
    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
];

/// The client's `rpc.client.call.duration` histogram.
#[derive(Clone, Debug)]
pub struct CallMetric(Histogram<f64>);

impl CallMetric {
    /// The histogram, from the global meter provider as it stands now.
    pub(crate) fn global() -> Self {
        Self::from_meter(&opentelemetry::global::meter("a2a.client"))
    }

    /// The histogram, from `meter`.
    pub(crate) fn from_meter(meter: &opentelemetry::metrics::Meter) -> Self {
        Self(
            meter
                .f64_histogram("rpc.client.call.duration")
                .with_description(
                    "Measures the duration of an outgoing Remote Procedure Call (RPC).",
                )
                .with_unit("s")
                .with_boundaries(BUCKETS.to_vec())
                .build(),
        )
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
        self.0.record(started.elapsed().as_secs_f64(), &attributes);
    }
}

#[cfg(test)]
mod tests;
