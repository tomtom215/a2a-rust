// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A meter whose measurements a test can read back.

use std::sync::{Arc, Weak};
use std::time::Duration;

use opentelemetry::metrics::{Meter, MeterProvider as _};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::metrics::data::ResourceMetrics;
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::metrics::{
    InstrumentKind, ManualReader, Pipeline, SdkMeterProvider, Temporality,
};

/// `SdkMeterProvider::with_reader` takes ownership, so the reader is shared
/// through an `Arc` to stay readable after the provider is built.
#[derive(Debug, Clone)]
pub struct SharedReader(pub Arc<ManualReader>);

impl MetricReader for SharedReader {
    fn register_pipeline(&self, pipeline: Weak<Pipeline>) {
        self.0.register_pipeline(pipeline);
    }
    fn collect(&self, rm: &mut ResourceMetrics) -> OTelSdkResult {
        self.0.collect(rm)
    }
    fn force_flush(&self) -> OTelSdkResult {
        self.0.force_flush()
    }
    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.0.shutdown_with_timeout(timeout)
    }
    fn temporality(&self, kind: InstrumentKind) -> Temporality {
        self.0.temporality(kind)
    }
}

/// A meter over a provider of its own, not the global one, so tests running
/// in parallel cannot see each other's measurements; the reader to collect
/// them; and the provider, which must outlive the measurements.
#[cfg_attr(not(feature = "tool-loop"), allow(dead_code))]
pub fn recording_meter() -> (Meter, Arc<ManualReader>, SdkMeterProvider) {
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    (provider.meter("a2a-test"), reader, provider)
}

/// Everything recorded so far.
#[cfg_attr(not(feature = "tool-loop"), allow(dead_code))]
pub fn collect(reader: &ManualReader) -> ResourceMetrics {
    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    collected
}
