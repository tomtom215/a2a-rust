// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! W3C Baggage on outbound calls, and `OTEL_PROPAGATORS` (audit C-10; ADR
//! 0038 §1).
//!
//! **Opt-in, by key.** Baggage is a data channel to whoever the next hop is,
//! and an A2A peer is often another organisation's agent: forwarding every
//! entry would hand it whatever an upstream service put there (`user.id`,
//! `tenant`, a session). So nothing is sent until
//! [`ClientConfig::with_baggage_propagation`] names the keys that may cross
//! ([`BaggagePolicy::keys`]), or, knowingly, all of them
//! ([`BaggagePolicy::All`]). What is sent is bounded by the W3C limits (64
//! members, 8192 bytes, whole members only) and is the baggage of the
//! OpenTelemetry context the call runs in.
//!
//! **`OTEL_PROPAGATORS`.** Read once, when the [`ClientConfig`] is made
//! (default `tracecontext,baggage`): without `tracecontext` the call writes no
//! `traceparent` of its own, without `baggage` no `baggage`; `none` turns off
//! both. A name this crate does not carry is warned about when the client is
//! built and ignored; a list of only such names gets the default.
//!
//! Needs the `otel` feature, a `tracing-opentelemetry` layer recording, and a
//! transport with per-request headers (JSON-RPC, HTTP+JSON, gRPC), as the
//! `traceparent` does.

pub use a2a_protocol_types::trace_context::baggage::{BaggagePolicy, Propagators};

use crate::config::ClientConfig;

impl ClientConfig {
    /// Which baggage entries this client sends; [`BaggagePolicy::Off`] by
    /// default. See the [module](crate::otel_baggage) for why it is opt-in.
    #[must_use]
    pub fn with_baggage_propagation(mut self, policy: BaggagePolicy) -> Self {
        self.baggage = policy;
        self
    }

    /// Which propagators the calls use, instead of what `OTEL_PROPAGATORS`
    /// said when this config was made.
    #[must_use]
    pub fn with_propagators(mut self, propagators: Propagators) -> Self {
        self.propagators = propagators;
        self
    }
}

/// Warns, once per client built, of each `OTEL_PROPAGATORS` name this crate
/// does not carry (decision F3: a value that cannot be used warns, and the
/// rest of the list, or the default, applies).
pub(crate) fn warn_unknown(propagators: &Propagators) {
    for _name in &propagators.unknown {
        trace_warn!(
            propagator = %_name,
            "OTEL_PROPAGATORS names a propagator this client does not carry; ignored \
             (it carries tracecontext and baggage)"
        );
    }
}

/// Writes the entries of the call span's baggage that `policy` lets out as
/// the call's `baggage` header, unless the caller set one (in any case).
#[cfg(feature = "otel")]
pub(crate) fn propagate(
    span: &tracing::Span,
    carries_headers: bool,
    policy: &BaggagePolicy,
    headers: &mut std::collections::HashMap<String, String>,
) {
    use a2a_protocol_types::trace_context::baggage::{BAGGAGE_HEADER, encode};
    use opentelemetry::baggage::BaggageExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;
    // `Off` needs no check of its own: it allows no key, so `encode` below
    // writes nothing.
    if !carries_headers
        || span.is_disabled()
        || headers
            .keys()
            .any(|k| k.eq_ignore_ascii_case(BAGGAGE_HEADER))
    {
        return;
    }
    let context = span.context();
    let baggage = context.baggage();
    let members: Vec<(&str, &str, Option<&str>)> = baggage
        .iter()
        .map(|(key, (value, metadata))| (key.as_str(), value.as_str(), Some(metadata.as_str())))
        .collect();
    if let Some(header) = encode(members, policy) {
        headers.insert(BAGGAGE_HEADER.to_owned(), header);
    }
}

#[cfg(all(test, feature = "otel"))]
mod tests;
