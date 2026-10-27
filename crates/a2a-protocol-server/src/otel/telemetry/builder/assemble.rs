// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `TelemetryBuilder::build`: one provider per signal that has an exporter,
//! an OTLP exporter and/or the console one, and a line saying where each
//! signal goes (`Telemetry::describe`).

use std::fmt::Write as _;

use opentelemetry_sdk::metrics::PeriodicReader;

use super::super::console::Console;
use super::super::propagators::Propagators;
use super::super::selection::Selection;
use super::*;

/// The metric reader's export interval, as `PeriodicReader` reads it.
fn metric_interval_ms(env: &Env) -> u64 {
    env("OTEL_METRIC_EXPORT_INTERVAL")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(60_000)
}

/// `traces → otlp http/protobuf http://c:4318/v1/traces, gzip + console`.
fn signal_line(signal: Signal, parts: &[String]) -> String {
    if parts.is_empty() {
        format!("{} → off", signal.name())
    } else {
        format!("{} → {}", signal.name(), parts.join(" + "))
    }
}

/// How an OTLP exporter is described: protocol, destination, and what is
/// done to the body or the route.
fn otlp_part(target: &Target) -> String {
    let mut part = format!("otlp {} {}", target.protocol.label(), target.destination);
    if target.compression.is_some() {
        part.push_str(", gzip");
    }
    if let Some(proxy) = target.proxy_display() {
        let _ = write!(part, ", via proxy {proxy}");
    }
    part
}

impl TelemetryBuilder {
    /// Builds every signal that has an exporter, and its provider.
    ///
    /// # Errors
    ///
    /// [`TelemetryError`] for a variable with a value this crate cannot act
    /// on — named, with the value, except for headers — an exporter that
    /// cannot be built, a CA file that cannot be read, or an export thread
    /// that cannot be started. Nothing is installed on error.
    pub fn build(self) -> Result<Telemetry, TelemetryError> {
        let chosen = [
            self.selection(Signal::Traces)?,
            self.selection(Signal::Metrics)?,
            self.selection(Signal::Logs)?,
        ];
        let mut telemetry = Telemetry {
            tracer: None,
            meter: None,
            logger: None,
            log_level: self.log_level,
            runtime: None,
            failures: Vec::new(),
            disabled: config::sdk_disabled(&*self.env),
            startup_summary: self.startup_summary,
            logs_on_console: chosen[2].console,
            report: Vec::new(),
            notices: Vec::new(),
            #[cfg(feature = "otel-prometheus")]
            prometheus: None,
        };
        // The export thread exists only when something is sent to a
        // collector or served for a scraper; a console-only process starts
        // none.
        let runtime = if chosen.iter().any(|s| s.needs_runtime()) {
            Some(ExportRuntime::start().map_err(TelemetryError::Runtime)?)
        } else {
            None
        };
        let (resource, resource_notices) = self.resource_and_notices();
        telemetry.notices.extend(resource_notices);
        let [traces, metrics, logs] = chosen;
        telemetry.tracer =
            self.tracer_provider(&mut telemetry, traces, runtime.as_ref(), &resource)?;
        telemetry.meter =
            self.meter_provider(&mut telemetry, metrics, runtime.as_ref(), &resource)?;
        telemetry.logger =
            self.logger_provider(&mut telemetry, logs, runtime.as_ref(), &resource)?;
        telemetry.runtime = runtime;
        telemetry.notices.extend(self.base_endpoint_notice([
            chosen[0].otlp,
            chosen[1].otlp,
            chosen[2].otlp,
        ]));
        let propagators = Propagators::from_env(&*self.env);
        telemetry.notices.extend(propagators.notice());
        if self.install_globally {
            if let Some(tracer) = &telemetry.tracer {
                opentelemetry::global::set_tracer_provider(tracer.clone());
            }
            if let Some(meter) = &telemetry.meter {
                opentelemetry::global::set_meter_provider(meter.clone());
            }
            opentelemetry::global::set_text_map_propagator(propagators.composite());
        }
        Ok(telemetry)
    }

    fn tracer_provider(
        &self,
        telemetry: &mut Telemetry,
        chosen: Selection,
        runtime: Option<&ExportRuntime>,
        resource: &Resource,
    ) -> Result<Option<SdkTracerProvider>, TelemetryError> {
        let mut parts = Vec::new();
        let mut builder = SdkTracerProvider::builder().with_resource(resource.clone());
        if chosen.otlp
            && let Some(runtime) = runtime
        {
            let target = self.target(Signal::Traces)?;
            let exporter = exporters::spans(&target, runtime)?;
            builder =
                builder.with_batch_exporter(telemetry.counting(Signal::Traces, exporter, &target));
            parts.push(otlp_part(&target));
        }
        if chosen.console {
            builder = builder.with_simple_exporter(Console::stdout());
            parts.push("console".to_owned());
        }
        telemetry.report.push(signal_line(Signal::Traces, &parts));
        Ok(chosen.any().then(|| builder.build()))
    }

    fn meter_provider(
        &self,
        telemetry: &mut Telemetry,
        chosen: Selection,
        runtime: Option<&ExportRuntime>,
        resource: &Resource,
    ) -> Result<Option<SdkMeterProvider>, TelemetryError> {
        let mut parts = Vec::new();
        let mut builder = SdkMeterProvider::builder().with_resource(resource.clone());
        if chosen.otlp
            && let Some(runtime) = runtime
        {
            let target = self.target(Signal::Metrics)?;
            let exporter = exporters::metrics(&target, runtime)?;
            builder = builder.with_reader(
                PeriodicReader::builder(telemetry.counting(Signal::Metrics, exporter, &target))
                    .build(),
            );
            parts.push(otlp_part(&target));
        }
        #[cfg(feature = "otel-prometheus")]
        if chosen.prometheus
            && let Some(runtime) = runtime
        {
            let started = super::super::prometheus::start(&*self.env, runtime)?;
            builder = builder.with_reader(started.reader);
            parts.push(format!("prometheus http://{}/metrics", started.addr));
            telemetry.prometheus = Some(started.addr);
        }
        if chosen.console {
            builder = builder.with_reader(PeriodicReader::builder(Console::stdout()).build());
            let ms = metric_interval_ms(&*self.env);
            let every = if ms.is_multiple_of(1_000) {
                format!("{} s", ms / 1_000)
            } else {
                format!("{ms} ms")
            };
            parts.push(format!("console every {every}"));
        }
        telemetry.report.push(signal_line(Signal::Metrics, &parts));
        Ok(chosen.any().then(|| builder.build()))
    }

    fn logger_provider(
        &self,
        telemetry: &mut Telemetry,
        chosen: Selection,
        runtime: Option<&ExportRuntime>,
        resource: &Resource,
    ) -> Result<Option<SdkLoggerProvider>, TelemetryError> {
        let mut parts = Vec::new();
        let mut builder = SdkLoggerProvider::builder().with_resource(resource.clone());
        if chosen.otlp
            && let Some(runtime) = runtime
        {
            let target = self.target(Signal::Logs)?;
            let exporter = exporters::logs(&target, runtime)?;
            builder =
                builder.with_batch_exporter(telemetry.counting(Signal::Logs, exporter, &target));
            parts.push(otlp_part(&target));
        }
        if chosen.console {
            builder = builder.with_simple_exporter(Console::stdout());
            parts.push("console".to_owned());
        }
        telemetry.report.push(signal_line(Signal::Logs, &parts));
        Ok(chosen.any().then(|| builder.build()))
    }
}
