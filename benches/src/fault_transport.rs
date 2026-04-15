// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fault-injecting [`Transport`] wrapper for agent-level fault benchmarks.
//!
//! This module exists because the existing 267 benchmarks in this crate all
//! measure transport/protocol overhead at the *SDK* layer — request encode,
//! wire round-trip, task store contention, etc. None of them measure the
//! characteristic a reviewer of an agent harness actually wants: *end-to-end
//! latency of a multi-hop agent chain when the links between hops are
//! unreliable.* That shape of benchmark is on the feedback list as the
//! single highest-leverage "wrong kind of benchmark" fix.
//!
//! # What this does
//!
//! [`FaultInjectingTransport`] wraps any [`Transport`] implementation and,
//! before each delegated call, optionally:
//!
//! 1. Sleeps for a configurable per-hop latency.
//! 2. Returns a synthetic [`ClientError::Timeout`] based on a configurable
//!    error rate.
//!
//! Both faults are applied to [`Transport::send_request`] and
//! [`Transport::send_streaming_request`] alike, so it can sit between an
//! [`a2a_protocol_client::A2aClient`] and an underlying
//! [`a2a_protocol_client::transport::JsonRpcTransport`] with no other
//! changes.
//!
//! # Determinism
//!
//! The error-rate decision uses a per-instance counter fed through a simple
//! xorshift64 PRNG. The counter is atomic but starts from `0`, and the PRNG
//! seed is explicit, so two runs of the same benchmark with the same rate
//! get the same sequence of error decisions. That is what criterion needs to
//! produce stable statistical estimates — randomised error timing would
//! show up as enormous variance.
//!
//! # Honesty about what this is not
//!
//! This is in-process fault injection, not real packet loss. "1% error rate"
//! here means "1% of `send_request` calls return a synthetic `Timeout`
//! variant without touching the network," not "1% of TCP segments are
//! dropped on the wire." The observable effect on the *SDK's* retry and
//! timeout paths is the same; the observable effect on transport-level
//! congestion control is not. The benchmark documentation in
//! `book/src/reference/benchmarks.md` spells this out explicitly so the
//! numbers aren't over-read.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2a_protocol_client::error::{ClientError, ClientResult};
use a2a_protocol_client::streaming::EventStream;
use a2a_protocol_client::transport::Transport;

// ── FaultInjectingTransport ──────────────────────────────────────────────────

/// A [`Transport`] wrapper that injects synthetic latency and error faults.
///
/// Wraps any inner transport (typically
/// [`a2a_protocol_client::transport::JsonRpcTransport`]) and delegates every
/// call after optionally:
///
/// - sleeping for [`Self::with_latency`]; and
/// - returning a synthetic [`ClientError::Timeout`] based on
///   [`Self::with_error_rate`].
///
/// The error-rate decision is deterministic per instance: the same counter
/// sequence runs through an xorshift64 PRNG seeded by `seed`. Criterion
/// benches therefore see a stable sequence of fault decisions.
pub struct FaultInjectingTransport<T: Transport> {
    inner: T,
    latency: Duration,
    /// Error rate in basis points (0..=10_000).
    error_rate_bp: u64,
    counter: AtomicU64,
    seed: u64,
}

impl<T: Transport> FaultInjectingTransport<T> {
    /// Creates a new fault-injecting wrapper with no faults configured.
    ///
    /// Call [`Self::with_latency`] and/or [`Self::with_error_rate`] to
    /// configure the faults. With neither set, this is a transparent pass-
    /// through.
    #[must_use]
    pub const fn new(inner: T) -> Self {
        Self {
            inner,
            latency: Duration::ZERO,
            error_rate_bp: 0,
            counter: AtomicU64::new(0),
            seed: 0x51ED_CAFE_5EED_1337,
        }
    }

    /// Sets the per-request latency added before delegation.
    #[must_use]
    pub const fn with_latency(mut self, latency: Duration) -> Self {
        self.latency = latency;
        self
    }

    /// Sets the synthetic error rate, clamped to `0.0..=1.0`.
    ///
    /// `0.0` means "never inject errors," `1.0` means "always inject
    /// errors," `0.02` means "inject an error on ~2% of calls." Conversion
    /// is to basis points (1 bp = 0.01%).
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn with_error_rate(mut self, rate: f64) -> Self {
        let clamped = rate.clamp(0.0, 1.0);
        self.error_rate_bp = (clamped * 10_000.0) as u64;
        self
    }

    /// Overrides the PRNG seed used for error-rate decisions.
    ///
    /// Exposed for tests that want to verify the deterministic sequence
    /// from a known starting point.
    #[must_use]
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Returns `true` if this call should get a synthetic fault.
    ///
    /// The decision is a function of the call counter and the instance
    /// seed, so it is reproducible.
    fn should_inject_error(&self) -> bool {
        if self.error_rate_bp == 0 {
            return false;
        }
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        // xorshift64
        let mut x = n.wrapping_add(self.seed).wrapping_add(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % 10_000) < self.error_rate_bp
    }

    /// Applies the configured fault schedule: sleep, then optionally error.
    ///
    /// Returns `Ok(())` to continue to the inner transport, or
    /// `Err(ClientError::Timeout(...))` to short-circuit.
    async fn apply_fault(&self) -> ClientResult<()> {
        if !self.latency.is_zero() {
            tokio::time::sleep(self.latency).await;
        }
        if self.should_inject_error() {
            // Timeout is chosen deliberately: it is the ClientError variant
            // whose `is_retryable()` returns true, which matches the
            // semantics of a real transient network fault and lets the
            // coordinator's retry loop exercise its retry path.
            return Err(ClientError::Timeout(
                "fault_transport: synthetic fault for benchmark".into(),
            ));
        }
        Ok(())
    }
}

impl<T: Transport> Transport for FaultInjectingTransport<T> {
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        Box::pin(async move {
            self.apply_fault().await?;
            self.inner.send_request(method, params, extra_headers).await
        })
    }

    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async move {
            self.apply_fault().await?;
            self.inner
                .send_streaming_request(method, params, extra_headers)
                .await
        })
    }
}
