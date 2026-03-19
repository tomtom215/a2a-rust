// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! OTLP metrics export pipeline initialization.
//!
//! Configures a periodic reader that exports metrics via gRPC to an
//! OTLP-compatible collector (e.g. the OpenTelemetry Collector, Grafana
//! Alloy, or Datadog Agent).

use opentelemetry_sdk::metrics::SdkMeterProvider;

/// Initialise an OTLP metrics export pipeline and install it as the global
/// [`MeterProvider`].
///
/// The endpoint defaults to `http://localhost:4317` and can be overridden via
/// the `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable.
///
/// Returns the [`SdkMeterProvider`] so the caller can hold onto it and call
/// [`SdkMeterProvider::shutdown`] during graceful termination.
///
/// # Arguments
///
/// * `service_name` — value for the `service.name` resource attribute.
///
/// # Errors
///
/// Returns an error if the OTLP exporter or meter provider cannot be created.
///
/// [`MeterProvider`]: opentelemetry::metrics::MeterProvider
pub fn init_otlp_pipeline(
    service_name: &str,
) -> Result<SdkMeterProvider, Box<dyn std::error::Error>> {
    use opentelemetry::KeyValue as Kv;
    use opentelemetry_otlp::MetricExporter;
    use opentelemetry_sdk::metrics::PeriodicReader;
    use opentelemetry_sdk::Resource;

    let exporter = MetricExporter::builder().with_tonic().build()?;

    let reader = PeriodicReader::builder(exporter).build();

    let resource = Resource::builder()
        .with_attributes([Kv::new("service.name", service_name.to_owned())])
        .build();

    let provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource)
        .build();

    opentelemetry::global::set_meter_provider(provider.clone());

    Ok(provider)
}
