// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`TelemetryBuilder`]: what to export, where, and how, read from the
//! environment and overridden in code.

use super::selection::{self, Selection};
use super::*;

mod assemble;

/// Configures and builds a [`Telemetry`]; see [`Telemetry::builder`].
#[allow(clippy::struct_excessive_bools)] // independent switches, each set by its own method
pub struct TelemetryBuilder {
    default_service_name: Option<String>,
    traces: Option<bool>,
    metrics: Option<bool>,
    logs: Option<bool>,
    log_level: tracing::Level,
    endpoint: Option<String>,
    protocol: Option<OtlpProtocol>,
    headers: Vec<(String, String)>,
    compression: Option<OtlpCompression>,
    proxy: ProxySetting,
    detect_resource: bool,
    startup_summary: bool,
    ca_pem: Vec<u8>,
    client_identity: Option<ClientIdentity>,
    langfuse: Option<Langfuse>,
    install_globally: bool,
    where_configured: bool,
    env: Arc<Env>,
}

impl std::fmt::Debug for TelemetryBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Headers and the proxy URL are not printed: they carry credentials.
        f.debug_struct("TelemetryBuilder")
            .field("default_service_name", &self.default_service_name)
            .field("traces", &self.traces)
            .field("metrics", &self.metrics)
            .field("logs", &self.logs)
            .field("endpoint", &self.endpoint)
            .field("protocol", &self.protocol)
            .field("compression", &self.compression)
            .field("langfuse", &self.langfuse)
            .field("where_configured", &self.where_configured)
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
            compression: None,
            proxy: ProxySetting::default(),
            detect_resource: true,
            startup_summary: true,
            ca_pem: Vec::new(),
            client_identity: None,
            langfuse: None,
            install_globally: true,
            where_configured: false,
            env,
        }
    }

    /// The `service.name` to report when the environment names none.
    /// `OTEL_SERVICE_NAME`, or `service.name` in `OTEL_RESOURCE_ATTRIBUTES`,
    /// wins over it, as the OpenTelemetry environment specification has it —
    /// unlike [`init_otlp_pipeline`](crate::otel::init_otlp_pipeline)'s argument.
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

    /// Exports each signal only where an endpoint is configured for it: in
    /// code ([`with_otlp_endpoint`](Self::with_otlp_endpoint), or
    /// [`with_langfuse`](Self::with_langfuse) for traces), or in
    /// `OTEL_EXPORTER_OTLP_ENDPOINT`, or in the signal's own
    /// `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` /
    /// `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` /
    /// `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`. A signal with none is not built:
    /// no exporter, no export thread. For a binary that must run as well
    /// with a collector as without one; without this, every signal is
    /// exported, to `localhost` when nothing names an endpoint, as the
    /// OpenTelemetry environment specification has it.
    ///
    /// [`with_traces`](Self::with_traces) and its siblings, set in code,
    /// still win, and `OTEL_TRACES_EXPORTER` and its siblings win over both.
    #[must_use]
    pub const fn with_export_where_configured(mut self) -> Self {
        self.where_configured = true;
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

    /// Compresses every export body. `OTEL_EXPORTER_OTLP_COMPRESSION` (and
    /// the per-signal forms) asks for the same from the environment, and
    /// this wins over it. Code cannot turn off what the variable turned on:
    /// unset the variable, or set it to `none`. Without either, bodies are
    /// sent as they are.
    #[must_use]
    pub const fn with_compression(mut self, compression: OtlpCompression) -> Self {
        self.compression = Some(compression);
        self
    }

    /// Sends every export through this HTTP proxy, as `ALL_PROXY` would, and
    /// wins over `ALL_PROXY`, `HTTPS_PROXY` and `HTTP_PROXY`. `NO_PROXY` does
    /// not apply to it: a proxy chosen in code is explicit, and does not
    /// change with the machine's environment. Credentials go in the URL
    /// (`http://user:pass@proxy:3128`). Only `http://` proxies are spoken; an
    /// `https://` or `socks5://` one is ignored with a warning, and the
    /// export goes direct.
    #[must_use]
    pub fn with_proxy(mut self, url: impl Into<String>) -> Self {
        self.proxy = ProxySetting::Url(url.into());
        self
    }

    /// Sends exports straight to the collector, ignoring `HTTP_PROXY`,
    /// `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY`. Without this, and without
    /// [`with_proxy`](Self::with_proxy), those variables apply as they do to
    /// curl.
    #[must_use]
    pub fn without_proxy(mut self) -> Self {
        self.proxy = ProxySetting::Off;
        self
    }

    /// Leaves the process's own description out of the resource. By default
    /// it carries `host.name`, `host.arch`, `os.type`, `process.pid` and
    /// `process.executable.name`, each only when `OTEL_RESOURCE_ATTRIBUTES`
    /// does not name it; turn this off for an export to a party that should
    /// not learn them. `service.name`, `service.instance.id` and
    /// `telemetry.sdk.*` stay.
    #[must_use]
    pub const fn without_resource_detection(mut self) -> Self {
        self.detect_resource = false;
        self
    }

    /// Leaves the startup line (`describe()`) out of what
    /// [`Telemetry::install_subscriber`] logs. By default it logs where each
    /// signal goes, or that nothing is exported and how to change that, once
    /// at INFO: what a server's operator wants and a one-shot command-line tool
    /// does not, where it would be a line on standard error of every run.
    /// Warnings and errors (a lost export, a setting that was not honoured)
    /// are still printed, and `describe()` still returns the line.
    #[must_use]
    pub const fn without_startup_summary(mut self) -> Self {
        self.startup_summary = false;
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

    /// A client certificate chain (leaf first) and its private key, PEM,
    /// presented to a collector that requires mutual TLS, over either
    /// transport. `OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE` and
    /// `OTEL_EXPORTER_OTLP_CLIENT_KEY`, or their per-signal forms, name files
    /// of them and win over this, as a variable wins over a header set in
    /// code. Both are checked when the exporters are built: a certificate or
    /// key that does not parse, or a key that is not the certificate's on
    /// OTLP/HTTP, fails [`build`](Self::build).
    #[must_use]
    pub fn with_client_certificate_pem(
        mut self,
        cert_chain: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> Self {
        self.client_identity = Some(ClientIdentity {
            cert_pem: cert_chain.as_ref().to_vec(),
            key_pem: key.as_ref().to_vec(),
        });
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

    fn selection(&self, signal: Signal) -> Result<Selection, TelemetryError> {
        if config::sdk_disabled(&*self.env) {
            return Ok(Selection::OFF);
        }
        let (explicit, preset_default) = match signal {
            Signal::Traces => (self.traces, true),
            Signal::Metrics => (self.metrics, self.langfuse.is_none()),
            Signal::Logs => (self.logs, self.langfuse.is_none()),
        };
        let default = if self.where_configured {
            let in_code =
                self.endpoint.is_some() || (signal == Signal::Traces && self.langfuse.is_some());
            preset_default && (in_code || signal.endpoint_in_env(&*self.env))
        } else {
            preset_default
        };
        selection::from_env(&*self.env, signal, explicit.unwrap_or(default))
    }

    /// OTLP/HTTP's endpoint must carry a scheme. `opentelemetry-otlp` answers
    /// `127.0.0.1:4318` with "invalid format", naming nothing to change; the
    /// gRPC path accepts it (`https://`, or `http://` under
    /// `OTEL_EXPORTER_OTLP_INSECURE`), but OTLP/HTTP has no such variable, so
    /// guessing `http` or `https` for the user would be a guess about TLS.
    fn require_scheme(&self, signal: Signal) -> Result<(), TelemetryError> {
        let given = self.endpoint.as_deref().map_or_else(
            || signal.otlp_var(&*self.env, "ENDPOINT"),
            |base| Some(("the endpoint given in code".to_owned(), base.to_owned())),
        );
        match given {
            Some((var, value)) if !value.contains("://") => Err(TelemetryError::config(
                &var,
                &value,
                format!("OTLP/HTTP needs a scheme: write `http://{value}` or `https://{value}`"),
            )),
            _ => Ok(()),
        }
    }

    /// A warning when the base endpoint over OTLP/HTTP already ends in one of
    /// the paths the exporter appends to it.
    ///
    /// `OTEL_EXPORTER_OTLP_ENDPOINT` is a base: the specification has the
    /// exporter add `/v1/traces`, `/v1/metrics` or `/v1/logs`, so a value
    /// copied from a signal's full URL (`http://c:4318/v1/traces`) posts to
    /// `/v1/traces/v1/traces`, which a collector answers with a 404. The
    /// signal's own `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` is the full URL, and
    /// is not checked. `otlp` says which signals go over OTLP.
    pub(super) fn base_endpoint_notice(&self, otlp: [bool; 3]) -> Option<String> {
        const SIGNALS: [Signal; 3] = [Signal::Traces, Signal::Metrics, Signal::Logs];
        const PATHS: [&str; 3] = ["/v1/traces", "/v1/metrics", "/v1/logs"];
        for (signal, over_otlp) in SIGNALS.into_iter().zip(otlp) {
            let Ok(protocol) = config::protocol(&*self.env, signal, self.protocol) else {
                continue;
            };
            if !over_otlp || !protocol.is_http() {
                continue;
            }
            let (origin, base) = match self.endpoint.as_deref() {
                Some(base) => ("the endpoint given in code".to_owned(), base.to_owned()),
                None => match signal.otlp_var(&*self.env, "ENDPOINT") {
                    Some((var, value)) if var == "OTEL_EXPORTER_OTLP_ENDPOINT" => (var, value),
                    _ => continue,
                },
            };
            let trimmed = base.trim_end_matches('/');
            let Some(suffix) = PATHS.iter().find(|p| trimmed.ends_with(**p)) else {
                continue;
            };
            let root = &trimmed[..trimmed.len() - suffix.len()];
            return Some(format!(
                "{origin} is {base}, which already ends in {suffix}: over OTLP/HTTP the exporter \
                 appends /v1/traces, /v1/metrics or /v1/logs to it, so {} would go to {}. Give the \
                 base URL ({root}), or a complete URL in OTEL_EXPORTER_OTLP_{}_ENDPOINT.",
                signal.name(),
                Target::endpoint_from_base(&base, signal, protocol),
                signal.name().to_uppercase(),
            ));
        }
        None
    }

    /// The client certificate and key the environment names for `signal`,
    /// else the ones given to the builder.
    fn client_identity_for(
        &self,
        signal: Signal,
    ) -> Result<Option<ClientIdentity>, TelemetryError> {
        Ok(match config::client_identity_files(&*self.env, signal)? {
            Some([(cert_var, cert_path), (key_var, key_path)]) => Some(ClientIdentity {
                cert_pem: read_file(&cert_var, &cert_path)?,
                key_pem: read_file(&key_var, &key_path)?,
            }),
            None => self.client_identity.clone(),
        })
    }

    pub(super) fn target(&self, signal: Signal) -> Result<Target, TelemetryError> {
        let env = &*self.env;
        let mut ca_pem = self.ca_pem.clone();
        if let Some((var, path)) = config::certificate_file(env, signal) {
            ca_pem.extend_from_slice(&read_file(&var, &path)?);
        }
        let client_identity = self.client_identity_for(signal)?;
        let timeout = config::timeout(env, signal)?;
        let proxy = Arc::new(proxy::matcher(env, &self.proxy));
        let compression = match self.compression {
            Some(c) => Some(c),
            None => compression::from_env(env, signal)?,
        };
        if let (Signal::Traces, Some(lf)) = (signal, &self.langfuse) {
            let endpoint = lf.traces_endpoint();
            return Ok(Target {
                signal,
                protocol: OtlpProtocol::HttpProtobuf,
                destination: Target::destination_for(
                    env,
                    signal,
                    OtlpProtocol::HttpProtobuf,
                    Some(&endpoint),
                ),
                endpoint: Some(endpoint),
                headers: lf.headers(),
                compression,
                proxy,
                cause: Arc::default(),
                timeout,
                ca_pem,
                client_identity,
            });
        }
        let protocol = config::protocol(env, signal, self.protocol)?;
        if protocol.is_http() {
            self.require_scheme(signal)?;
        }
        // OTLP/HTTP leaves an unset endpoint to `opentelemetry-otlp`. gRPC
        // resolves it here, because an `https://` one is dialled over this
        // crate's own TLS connector, which needs the endpoint to build.
        let endpoint = match self.endpoint.as_deref() {
            Some(base) => Some(Target::endpoint_from_base(base, signal, protocol)),
            None if protocol == OtlpProtocol::Grpc => signal.grpc_endpoint_in_env(env),
            None => None,
        };
        Ok(Target {
            signal,
            protocol,
            destination: Target::destination_for(env, signal, protocol, endpoint.as_deref()),
            endpoint,
            headers: self.headers.clone(),
            compression,
            proxy,
            cause: Arc::default(),
            timeout,
            ca_pem,
            client_identity,
        })
    }

    #[cfg(test)]
    pub(super) fn resource(&self) -> Resource {
        self.resource_and_notices().0
    }

    /// The resource, with `OTEL_RESOURCE_ATTRIBUTES` percent-decoded as the
    /// specification requires (`resource_env.rs`), and what could not be.
    pub(super) fn resource_and_notices(&self) -> (Resource, Vec<String>) {
        let decoded = super::resource_env::decode(&*self.env);
        (
            super::resource_env::apply(self.sdk_resource(), decoded.attributes),
            decoded.notices,
        )
    }

    fn sdk_resource(&self) -> Resource {
        // `Resource::builder` reads `OTEL_SERVICE_NAME` and
        // `OTEL_RESOURCE_ATTRIBUTES` itself; the defaults are added only when
        // they name none, so they never overwrite them.
        let mut builder = Resource::builder();
        if let Some(name) = &self.default_service_name
            && !config::service_name_from_env(&*self.env)
        {
            builder = builder.with_service_name(name.clone());
        }
        // The conventions make `service.instance.id` the one attribute that
        // tells two replicas of a service apart (`service.instance.id`,
        // resource/service.md: "MUST be unique for each instance of the same
        // service.namespace,service.name pair"). Prometheus turns it into
        // `instance`, so without one every replica writes the same series.
        // A random UUID is what the conventions recommend when nothing
        // better names the process.
        if !config::resource_attribute_from_env(&*self.env, "service.instance.id") {
            builder = builder.with_attribute(opentelemetry::KeyValue::new(
                "service.instance.id",
                uuid::Uuid::new_v4().to_string(),
            ));
        }
        if self.detect_resource {
            for found in detect::attributes(&*self.env) {
                if !config::resource_attribute_from_env(&*self.env, found.key.as_str()) {
                    builder = builder.with_attribute(found);
                }
            }
        }
        builder.build()
    }
}

/// The contents of the file `path`, which variable `var` named.
fn read_file(var: &str, path: &str) -> Result<Vec<u8>, TelemetryError> {
    std::fs::read(path)
        .map_err(|e| TelemetryError::config(var, path, format!("cannot be read: {e}")))
}
