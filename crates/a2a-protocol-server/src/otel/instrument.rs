// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`GlobalInstrument`]: an instrument on the global meter provider, built
//! once per provider instead of once per measurement (audit C-23).
//!
//! The store, card, push and tool-loop instruments used to be built at every
//! measurement: `global::meter(..)` (a read lock on the global and a scope
//! lookup), then the instrument builder (an instrument lookup and dedupe in
//! the SDK, and a `Vec` of bucket boundaries cloned) — per database
//! operation, per card request, per push attempt. This keeps the instrument
//! for as long as the global provider is the one it was built from, and
//! builds it again from a new one when the process installs another, as
//! `GlobalOtelMetrics` and the client's `CallMetric` do. What is left per
//! measurement is the read of the global and a pointer comparison.

use std::sync::{Arc, RwLock};

use opentelemetry::metrics::{Meter, MeterProvider};

/// The provider an instrument was built from, kept so its address cannot be
/// reused by another provider and mistaken for it.
type Provider = Arc<dyn MeterProvider + Send + Sync>;

/// An instrument (or a set of them) of type `T`, built by `build` from the
/// `a2a.server` meter of the global provider as it stands at each use.
pub struct GlobalInstrument<T> {
    bound: RwLock<Option<(Provider, T)>>,
    build: fn(&Meter) -> T,
    /// Where the provider comes from: the global, except in this module's
    /// tests, which must not race the crate's other tests for it.
    source: fn() -> Provider,
}

/// The instrumentation scope every server instrument is recorded under.
const METER_NAME: &str = "a2a.server";

impl<T: Clone> GlobalInstrument<T> {
    /// Built from the global provider on first use, with `build`.
    pub const fn new(build: fn(&Meter) -> T) -> Self {
        Self {
            bound: RwLock::new(None),
            build,
            source: opentelemetry::global::meter_provider,
        }
    }

    /// The instrument for the provider that is global now.
    pub fn get(&self) -> T {
        let provider = (self.source)();
        if let Ok(bound) = self.bound.read()
            && let Some((built_from, instrument)) = bound.as_ref()
            && Arc::ptr_eq(built_from, &provider)
        {
            return instrument.clone();
        }
        let instrument = (self.build)(&provider.meter(METER_NAME));
        if let Ok(mut bound) = self.bound.write() {
            *bound = Some((provider, instrument.clone()));
        }
        instrument
    }
}

impl<T> std::fmt::Debug for GlobalInstrument<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalInstrument").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, PoisonError};

    // Each test has its own provider slot and build counter: nothing here is
    // shared with another test, so they may run in any order or at once.

    static PROVIDER_A: Mutex<Option<Provider>> = Mutex::new(None);
    static BUILDS_A: AtomicUsize = AtomicUsize::new(0);

    fn source_a() -> Provider {
        PROVIDER_A
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("the test sets a provider first")
    }

    fn build_a(meter: &Meter) -> opentelemetry::metrics::Counter<u64> {
        BUILDS_A.fetch_add(1, Ordering::SeqCst);
        meter.u64_counter("a2a.test.cached").build()
    }

    fn sdk() -> Provider {
        Arc::new(opentelemetry_sdk::metrics::SdkMeterProvider::builder().build())
    }

    #[test]
    fn an_instrument_is_built_once_per_provider_and_again_for_a_new_one() {
        let cached = GlobalInstrument {
            bound: RwLock::new(None),
            build: build_a,
            source: source_a,
        };
        let first = sdk();
        *PROVIDER_A.lock().unwrap() = Some(Arc::clone(&first));
        for _ in 0..5 {
            cached.get().add(1, &[]);
        }
        assert_eq!(BUILDS_A.load(Ordering::SeqCst), 1, "five uses, one build");
        let second = sdk();
        *PROVIDER_A.lock().unwrap() = Some(second);
        cached.get().add(1, &[]);
        cached.get().add(1, &[]);
        assert_eq!(
            BUILDS_A.load(Ordering::SeqCst),
            2,
            "a new provider is built for once"
        );
        *PROVIDER_A.lock().unwrap() = Some(first);
        cached.get().add(1, &[]);
        assert_eq!(
            BUILDS_A.load(Ordering::SeqCst),
            3,
            "only the last provider is kept: going back builds again"
        );
        assert!(format!("{cached:?}").starts_with("GlobalInstrument"));
    }

    static PROVIDER_B: Mutex<Option<Provider>> = Mutex::new(None);

    fn source_b() -> Provider {
        PROVIDER_B
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("the test sets a provider first")
    }

    /// What is recorded through the cached instrument reaches the provider
    /// that is current, under the `a2a.server` scope.
    #[test]
    fn measurements_reach_the_current_provider() {
        use opentelemetry_sdk::metrics::ManualReader;
        use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
        let cached = GlobalInstrument {
            bound: RwLock::new(None),
            build: |m: &Meter| m.u64_counter("a2a.test.reached").build(),
            source: source_b,
        };
        let reader = Arc::new(ManualReader::builder().build());
        let provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
            .with_reader(crate::otel::test_reader::SharedReader(Arc::clone(&reader)))
            .build();
        *PROVIDER_B.lock().unwrap() = Some(Arc::new(provider));
        cached.get().add(3, &[]);
        cached.get().add(4, &[]);
        let exported = crate::otel::test_reader::collect(&reader);
        let scope = exported
            .scope_metrics()
            .find(|s| s.scope().name() == METER_NAME)
            .expect("the a2a.server scope");
        let metric = scope
            .metrics()
            .find(|m| m.name() == "a2a.test.reached")
            .expect("the counter");
        let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
            panic!("a u64 sum");
        };
        assert_eq!(
            sum.data_points()
                .map(opentelemetry_sdk::metrics::data::SumDataPoint::value)
                .sum::<u64>(),
            7
        );
    }
}
