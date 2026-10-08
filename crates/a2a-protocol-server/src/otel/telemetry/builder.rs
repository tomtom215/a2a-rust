// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`TelemetryBuilder`]: what to export, where, and how, read from the
//! environment and overridden in code.

use super::*;

/// Configures and builds a [`Telemetry`]; see [`Telemetry::builder`].
pub struct TelemetryBuilder {
    default_service_name: Option<String>,
    traces: Option<bool>,
    metrics: Option<bool>,
    logs: Option<bool>,
    log_level: tracing::Level,
    endpoint: Option<String>,
    protocol: Option<OtlpProtocol>,
    headers: Vec<(String, String)>,
    ca_pem: Vec<u8>,
    langfuse: Option<Langfuse>,
    install_globally: bool,
    env: Arc<Env>,
}

impl std::fmt::Debug for TelemetryBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Headers are not printed: they carry credentials.
        f.debug_struct("TelemetryBuilder")
            .field("default_service_name", &self.default_service_name)
            .field("traces", &self.traces)
            .field("metrics", &self.metrics)
            .field("logs", &self.logs)
            .field("endpoint", &self.endpoint)
            .field("protocol", &self.protocol)
            .field("langfuse", &self.langfuse)
            .finish_non_exhaustive()
    }
}

impl TelemetryBuilder {
    pub(super) fn new(env: Arc<Env>) -> Self {
        Self {
            default_service_name: None,
            traces: None,
            metrics: None,
            logs: None,
            log_level: tracing::Level::INFO,
            endpoint: None,
            protocol: None,
            headers: Vec::new(),
            ca_pem: Vec::new(),
            langfuse: None,
            install_globally: true,
            env,
        }
    }

    /// The `service.name` to report when the environment names none.
    /// `OTEL_SERVICE_NAME`, or `service.name` in `OTEL_RESOURCE_ATTRIBUTES`,
    /// wins over it, as the OpenTelemetry environment specification has it —
    /// unlike [`init_otlp_pipeline`](super::init_otlp_pipeline)'s argument.
    #[must_use]
    pub fn with_default_service_name(mut self, name: impl Into<String>) -> Self {
        self.default_service_name = Some(name.into());
        self
    }

    /// Whether traces are exported when `OTEL_TRACES_EXPORTER` is unset
    /// (default: yes). The variable, `otlp` or `none`, wins.
    #[must_use]
    pub const fn with_traces(mut self, on: bool) -> Self {
        self.traces = Some(on);
        self
    }

    /// Whether metrics are exported when `OTEL_METRICS_EXPORTER` is unset
    /// (default: yes, or no with [`with_langfuse`](Self::with_langfuse)).
    #[must_use]
    pub const fn with_metrics(mut self, on: bool) -> Self {
        self.metrics = Some(on);
        self
    }

    /// Whether `tracing` events are exported as OpenTelemetry log records
    /// when `OTEL_LOGS_EXPORTER` is unset (default: yes, or no with
    /// [`with_langfuse`](Self::with_langfuse)).
    #[must_use]
    pub const fn with_logs(mut self, on: bool) -> Self {
        self.logs = Some(on);
        self
    }

    /// The least severe event exported as a log record (default `INFO`).
    /// Spans are not filtered by level here; a subscriber-wide filter, such
    /// as `tracing_subscriber::EnvFilter`, applies to both.
    #[must_use]
    pub const fn with_log_level(mut self, level: tracing::Level) -> Self {
        self.log_level = level;
        self
    }

    /// The collector's base URL for every signal, as
    /// `OTEL_EXPORTER_OTLP_ENDPOINT` would give it: OTLP/HTTP appends
    /// `/v1/traces`, `/v1/metrics` or `/v1/logs`. Set in code, it wins over
    /// the variable.
    #[must_use]
    pub fn with_otlp_endpoint(mut self, base_url: impl Into<String>) -> Self {
        self.endpoint = Some(base_url.into());
        self
    }

    /// The transport for every signal. Set in code, it wins over
    /// `OTEL_EXPORTER_OTLP_PROTOCOL`; unset, that variable decides, and
    /// HTTP/protobuf is the default.
    #[must_use]
    pub const fn with_otlp_protocol(mut self, protocol: OtlpProtocol) -> Self {
        self.protocol = Some(protocol);
        self
    }

    /// A header sent with every export, alongside
    /// `OTEL_EXPORTER_OTLP_HEADERS`.
    #[must_use]
    pub fn with_otlp_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// CA certificates, PEM, to trust alongside the bundled Mozilla roots —
    /// for a collector behind a private CA. `OTEL_EXPORTER_OTLP_CERTIFICATE`
    /// names a file of them and is read too.
    #[must_use]
    pub fn with_ca_certificate_pem(mut self, pem: impl AsRef<[u8]>) -> Self {
        self.ca_pem.extend_from_slice(pem.as_ref());
        self.ca_pem.push(b'\n');
        self
    }

    /// Sends traces to Langfuse; see [`Langfuse`] for exactly what it sets.
    #[must_use]
    pub fn with_langfuse(mut self, langfuse: Langfuse) -> Self {
        self.langfuse = Some(langfuse);
        self
    }

    /// Whether to install the tracer and meter providers, and the W3C
    /// `tracecontext` and `baggage` propagators, as the process-global ones
    /// (default: yes). Off, nothing global changes — for a test, or a
    /// process with providers of its own.
    #[must_use]
    pub const fn install_globally(mut self, on: bool) -> Self {
        self.install_globally = on;
        self
    }

    /// Reads variables from `env` instead of the process environment.
    #[cfg(test)]
    pub(super) fn with_env(
        mut self,
        env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        self.env = Arc::new(env);
        self
    }

    fn enabled(&self, signal: Signal) -> Result<bool, TelemetryError> {
        if config::sdk_disabled(&*self.env) {
            return Ok(false);
        }
        let (explicit, preset_default) = match signal {
            Signal::Traces => (self.traces, true),
            Signal::Metrics => (self.metrics, self.langfuse.is_none()),
            Signal::Logs => (self.logs, self.langfuse.is_none()),
        };
        config::exporter_enabled(&*self.env, signal, explicit.unwrap_or(preset_default))
    }

    pub(super) fn target(
        &self,
        signal: Signal,
        failures: Arc<AtomicU64>,
    ) -> Result<Target, TelemetryError> {
        let env = &*self.env;
        let mut ca_pem = self.ca_pem.clone();
        if let Some((var, path)) = config::certificate_file(env, signal) {
            let pem = std::fs::read(&path)
                .map_err(|e| TelemetryError::config(&var, &path, format!("cannot be read: {e}")))?;
            ca_pem.extend_from_slice(&pem);
        }
        let timeout = config::timeout(env, signal)?;
        if let (Signal::Traces, Some(lf)) = (signal, &self.langfuse) {
            return Ok(Target {
                signal,
                protocol: OtlpProtocol::HttpProtobuf,
                endpoint: Some(lf.traces_endpoint()),
                headers: lf.headers(),
                timeout,
                ca_pem,
                failures,
            });
        }
        let protocol = config::protocol(env, signal, self.protocol)?;
        Ok(Target {
            signal,
            protocol,
            endpoint: self
                .endpoint
                .as_deref()
                .map(|base| Target::endpoint_from_base(base, signal, protocol)),
            headers: self.headers.clone(),
            timeout,
            ca_pem,
            failures,
        })
    }

    pub(super) fn resource(&self) -> Resource {
        // `Resource::builder` reads `OTEL_SERVICE_NAME` and
        // `OTEL_RESOURCE_ATTRIBUTES` itself; the default is added only when
        // they name no service, so it never overwrites them.
        let builder = Resource::builder();
        match &self.default_service_name {
            Some(name) if !config::service_name_from_env(&*self.env) => {
                builder.with_service_name(name.clone()).build()
            }
            _ => builder.build(),
        }
    }

    /// Builds every enabled signal's exporter and provider.
    ///
    /// # Errors
    ///
    /// [`TelemetryError`] for a variable with a value this crate cannot act
    /// on — named, with the value, except for headers — an exporter that
    /// cannot be built, a CA file that cannot be read, or an export thread
    /// that cannot be started. Nothing is installed on error.
    pub fn build(self) -> Result<Telemetry, TelemetryError> {
        let traces = self.enabled(Signal::Traces)?;
        let metrics = self.enabled(Signal::Metrics)?;
        let logs = self.enabled(Signal::Logs)?;
        let mut telemetry = Telemetry {
            tracer: None,
            meter: None,
            logger: None,
            log_level: self.log_level,
            runtime: None,
            failures: Vec::new(),
        };
        if traces || metrics || logs {
            let runtime = ExportRuntime::start().map_err(TelemetryError::Runtime)?;
            let resource = self.resource();
            if traces {
                let failures = telemetry.counter(Signal::Traces);
                let exporter = exporters::spans(&self.target(Signal::Traces, failures)?, &runtime)?;
                telemetry.tracer = Some(
                    SdkTracerProvider::builder()
                        .with_batch_exporter(exporter)
                        .with_resource(resource.clone())
                        .build(),
                );
            }
            if metrics {
                let failures = telemetry.counter(Signal::Metrics);
                let exporter =
                    exporters::metrics(&self.target(Signal::Metrics, failures)?, &runtime)?;
                telemetry.meter = Some(
                    SdkMeterProvider::builder()
                        .with_reader(PeriodicReader::builder(exporter).build())
                        .with_resource(resource.clone())
                        .build(),
                );
            }
            if logs {
                let failures = telemetry.counter(Signal::Logs);
                let exporter = exporters::logs(&self.target(Signal::Logs, failures)?, &runtime)?;
                telemetry.logger = Some(
                    SdkLoggerProvider::builder()
                        .with_batch_exporter(exporter)
                        .with_resource(resource)
                        .build(),
                );
            }
            telemetry.runtime = Some(runtime);
        }
        if self.install_globally {
            if let Some(tracer) = &telemetry.tracer {
                opentelemetry::global::set_tracer_provider(tracer.clone());
            }
            if let Some(meter) = &telemetry.meter {
                opentelemetry::global::set_meter_provider(meter.clone());
            }
            opentelemetry::global::set_text_map_propagator(TextMapCompositePropagator::new(vec![
                Box::new(TraceContextPropagator::new()),
                Box::new(BaggagePropagator::new()),
            ]));
        }
        Ok(telemetry)
    }
}
