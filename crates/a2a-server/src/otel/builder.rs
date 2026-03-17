// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Builder for [`OtelMetrics`].

use super::OtelMetrics;

/// Default meter name used when none is specified via the builder.
const DEFAULT_METER_NAME: &str = "a2a.server";

/// Builder for [`OtelMetrics`].
///
/// Use [`OtelMetricsBuilder::new`] to create a builder, optionally configure
/// the meter name, then call [`build`](OtelMetricsBuilder::build) to obtain an
/// [`OtelMetrics`] instance.
#[derive(Debug, Clone)]
pub struct OtelMetricsBuilder {
    meter_name: &'static str,
}

impl Default for OtelMetricsBuilder {
    fn default() -> Self {
        Self {
            meter_name: DEFAULT_METER_NAME,
        }
    }
}

impl OtelMetricsBuilder {
    /// Create a new builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the OpenTelemetry meter name.
    ///
    /// Defaults to `"a2a.server"`.
    #[must_use]
    pub const fn meter_name(mut self, name: &'static str) -> Self {
        self.meter_name = name;
        self
    }

    /// Build the [`OtelMetrics`] instance.
    ///
    /// Instruments are created from the global [`MeterProvider`]. Make sure
    /// you have called [`init_otlp_pipeline`](super::init_otlp_pipeline) (or
    /// otherwise installed a `MeterProvider`) before calling this method.
    ///
    /// [`MeterProvider`]: opentelemetry::metrics::MeterProvider
    #[must_use]
    pub fn build(self) -> OtelMetrics {
        let meter = opentelemetry::global::meter(self.meter_name);
        OtelMetrics::from_meter(&meter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_default_meter_name() {
        let metrics = OtelMetricsBuilder::new().build();
        let debug = format!("{metrics:?}");
        assert!(debug.contains("OtelMetrics"));
    }

    #[test]
    fn builder_custom_meter_name() {
        let metrics = OtelMetricsBuilder::new().meter_name("custom.meter").build();
        let debug = format!("{metrics:?}");
        assert!(debug.contains("OtelMetrics"));
    }
}
