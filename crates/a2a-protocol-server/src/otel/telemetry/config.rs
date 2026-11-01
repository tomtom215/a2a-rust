// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How `Telemetry` reads the `OTEL_*` environment.
//!
//! Only what `opentelemetry-otlp` and `opentelemetry_sdk` do not already
//! read is read here: which signals to export, the transport (so the right
//! exporter is built), the export timeout and CA certificate (which the
//! exporter's HTTP client and TLS need up front), and whether the environment
//! names the service. Endpoints, headers, the sampler, the batch processor's
//! limits and `OTEL_RESOURCE_ATTRIBUTES` are left to those crates, which read
//! them as the specification defines.
//!
//! Every read goes through an [`Env`], so the rules are testable: writing the
//! process environment is `unsafe` in edition 2024, and this crate forbids
//! `unsafe`.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::TelemetryError;

// Where variables come from (the process environment, or a test's table) and
// how the process environment is read; shared with the outbound clients.
pub(super) use crate::net::proxy::{Env, process_env};

/// What the environment said that this crate did not act on, one line each,
/// and what it did instead.
///
/// The OpenTelemetry specification has an SDK warn and ignore a value it
/// cannot use, and a fleet shares one `OTEL_*` preset among SDKs that do not
/// all know the same exporters, so a process that refused to start over a
/// name it does not know (`zipkin`) took the whole fleet down for it (audit
/// C-08, maintainer's decision of 2026-11-01). `Telemetry` has no subscriber
/// yet when it parses, so the lines wait here for
/// [`Telemetry::notices`](super::Telemetry::notices) and
/// `install_subscriber`, which prints them at WARN. A line is kept once however
/// many signals read the variable.
#[derive(Debug, Default)]
pub(super) struct Notes(Mutex<Vec<String>>);

impl Notes {
    /// `variable` was set to `value`, which `why` says this crate cannot use;
    /// `instead` is what it did.
    pub(super) fn ignored(&self, variable: &str, value: &str, why: &str, instead: &str) {
        let line = format!("{variable}={value:?} {why}; {instead}");
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !lines.contains(&line) {
            lines.push(line);
        }
    }

    /// The lines so far, leaving none.
    pub(super) fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// One of the three signals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Signal {
    Traces,
    Metrics,
    Logs,
}

impl Signal {
    /// `traces`, `metrics` or `logs`.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Traces => "traces",
            Self::Metrics => "metrics",
            Self::Logs => "logs",
        }
    }

    /// `OTEL_{TRACES,METRICS,LOGS}_EXPORTER`.
    pub(super) const fn exporter_var(self) -> &'static str {
        match self {
            Self::Traces => "OTEL_TRACES_EXPORTER",
            Self::Metrics => "OTEL_METRICS_EXPORTER",
            Self::Logs => "OTEL_LOGS_EXPORTER",
        }
    }

    /// The infix of the signal-specific OTLP variables.
    const fn upper(self) -> &'static str {
        match self {
            Self::Traces => "TRACES",
            Self::Metrics => "METRICS",
            Self::Logs => "LOGS",
        }
    }

    /// The path OTLP/HTTP appends to a base endpoint for this signal.
    pub(super) const fn http_path(self) -> &'static str {
        match self {
            Self::Traces => "v1/traces",
            Self::Metrics => "v1/metrics",
            Self::Logs => "v1/logs",
        }
    }

    /// Whether an OTLP endpoint is configured for this signal in the
    /// environment: its own `OTEL_EXPORTER_OTLP_{SIGNAL}_ENDPOINT`, or the
    /// general `OTEL_EXPORTER_OTLP_ENDPOINT`.
    pub(super) fn endpoint_in_env(self, env: &Env) -> bool {
        self.otlp_var(env, "ENDPOINT").is_some()
    }

    /// The OTLP/gRPC endpoint the environment configures for this signal,
    /// as `opentelemetry-otlp` reads it: the signal's own variable, then the
    /// general one, used as given. One with no scheme gets `http://` when
    /// `OTEL_EXPORTER_OTLP_{SIGNAL_}INSECURE` is `true` and `https://`
    /// otherwise.
    pub(super) fn grpc_endpoint_in_env(self, env: &Env) -> Option<String> {
        let (_, endpoint) = self.otlp_var(env, "ENDPOINT")?;
        if endpoint.contains("://") {
            return Some(endpoint);
        }
        let insecure = self
            .otlp_var(env, "INSECURE")
            .is_some_and(|(_, v)| v.trim().eq_ignore_ascii_case("true"));
        let scheme = if insecure { "http" } else { "https" };
        Some(format!("{scheme}://{endpoint}"))
    }

    /// The signal-specific variable, falling back to the general one.
    pub(super) fn otlp_var(self, env: &Env, suffix: &str) -> Option<(String, String)> {
        let specific = format!("OTEL_EXPORTER_OTLP_{}_{suffix}", self.upper());
        if let Some(v) = env(&specific) {
            return Some((specific, v));
        }
        let general = format!("OTEL_EXPORTER_OTLP_{suffix}");
        env(&general).map(|v| (general, v))
    }
}

/// The OTLP transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OtlpProtocol {
    /// OTLP/gRPC, conventionally on port 4317.
    Grpc,
    /// OTLP/HTTP with protobuf bodies, conventionally on port 4318 — the
    /// specification's default, and the one Langfuse accepts.
    HttpProtobuf,
    /// OTLP/HTTP with JSON bodies, on the same port as protobuf. Needs the
    /// `otlp-json` feature.
    #[cfg(feature = "otlp-json")]
    HttpJson,
}

impl OtlpProtocol {
    /// The name `OTEL_EXPORTER_OTLP_PROTOCOL` gives it.
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Grpc => "grpc",
            Self::HttpProtobuf => "http/protobuf",
            #[cfg(feature = "otlp-json")]
            Self::HttpJson => "http/json",
        }
    }

    /// Whether the transport is OTLP/HTTP, whose endpoint is per signal.
    pub(super) const fn is_http(self) -> bool {
        !matches!(self, Self::Grpc)
    }
}

/// `OTEL_SDK_DISABLED=true` turns every signal off.
pub(super) fn sdk_disabled(env: &Env) -> bool {
    env("OTEL_SDK_DISABLED").is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}

/// A note when `OTEL_SDK_DISABLED` is set to something that is not `true` or
/// `false`: `1` and `yes` do not disable the SDK, and said nothing about it
/// (audit NEW-c-5). The specification reads only `true` (any case).
pub(super) fn note_misread_sdk_disabled(env: &Env, notes: &Notes) {
    const VAR: &str = "OTEL_SDK_DISABLED";
    if let Some(value) = env(VAR)
        && !["true", "false"]
            .iter()
            .any(|ok| value.trim().eq_ignore_ascii_case(ok))
    {
        notes.ignored(
            VAR,
            &value,
            "is not `true` or `false`, and only `true` disables the SDK",
            "the SDK is not disabled",
        );
    }
}

/// The transport for `signal`: `explicit` when given, else
/// `OTEL_EXPORTER_OTLP_{SIGNAL}_PROTOCOL` or `OTEL_EXPORTER_OTLP_PROTOCOL`
/// (matched without regard to case), else HTTP/protobuf, the specification's
/// default. A value that is none of the three is noted and HTTP/protobuf used.
pub(super) fn protocol(
    env: &Env,
    signal: Signal,
    explicit: Option<OtlpProtocol>,
    notes: &Notes,
) -> OtlpProtocol {
    if let Some(p) = explicit {
        return p;
    }
    let Some((var, value)) = signal.otlp_var(env, "PROTOCOL") else {
        return OtlpProtocol::HttpProtobuf;
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "grpc" => OtlpProtocol::Grpc,
        "http/protobuf" => OtlpProtocol::HttpProtobuf,
        "http/json" => http_json(notes, &var, &value),
        _ => {
            notes.ignored(
                &var,
                &value,
                "is not `grpc`, `http/protobuf` or `http/json`",
                "`http/protobuf` is used",
            );
            OtlpProtocol::HttpProtobuf
        }
    }
}

/// `http/json`, which only the `otlp-json` feature can export.
///
/// One function with the feature split inside it rather than two `#[cfg]`
/// match arms, for the reason `build_jwks_client` gives: under
/// `--all-features` the `not(otlp-json)` arm is compiled out, so deleting it
/// builds and passes every test, and the mutation gate reported it as a
/// survivor no test could reach.
#[cfg_attr(feature = "otlp-json", allow(clippy::missing_const_for_fn))]
fn http_json(notes: &Notes, var: &str, value: &str) -> OtlpProtocol {
    #[cfg(feature = "otlp-json")]
    {
        let _ = (notes, var, value);
        OtlpProtocol::HttpJson
    }
    #[cfg(not(feature = "otlp-json"))]
    {
        notes.ignored(
            var,
            value,
            "needs the `otlp-json` feature, which is not compiled in",
            "`http/protobuf` is used",
        );
        OtlpProtocol::HttpProtobuf
    }
}

/// The export timeout for `signal`, in milliseconds per the specification;
/// 10 s when unset, and when the value is not a number of milliseconds (noted).
pub(super) fn timeout(env: &Env, signal: Signal, notes: &Notes) -> Duration {
    const DEFAULT: Duration = Duration::from_secs(10);
    let Some((var, value)) = signal.otlp_var(env, "TIMEOUT") else {
        return DEFAULT;
    };
    value.trim().parse::<u64>().map_or_else(
        |_| {
            notes.ignored(
                &var,
                &value,
                "is not a number of milliseconds",
                "10000 is used",
            );
            DEFAULT
        },
        Duration::from_millis,
    )
}

/// The PEM file of extra CA certificates to trust for `signal`, if any:
/// `OTEL_EXPORTER_OTLP_{SIGNAL}_CERTIFICATE` or
/// `OTEL_EXPORTER_OTLP_CERTIFICATE`.
pub(super) fn certificate_file(env: &Env, signal: Signal) -> Option<(String, String)> {
    signal.otlp_var(env, "CERTIFICATE")
}

/// The client certificate and key files for mutual TLS with the collector,
/// if any, each as `(variable, path)`:
/// `OTEL_EXPORTER_OTLP_{SIGNAL}_CLIENT_CERTIFICATE` and `..._CLIENT_KEY`, each
/// falling back to its general variable on its own, as the specification
/// defines them.
///
/// # Errors
///
/// One without the other: a certificate cannot be presented without its key,
/// and a key alone names nothing to present, so either is a misconfiguration
/// that would otherwise connect without a client certificate and say nothing.
pub(super) fn client_identity_files(
    env: &Env,
    signal: Signal,
) -> Result<Option<[(String, String); 2]>, TelemetryError> {
    match (
        signal.otlp_var(env, "CLIENT_CERTIFICATE"),
        signal.otlp_var(env, "CLIENT_KEY"),
    ) {
        (None, None) => Ok(None),
        (Some(cert), Some(key)) => Ok(Some([cert, key])),
        (Some((var, value)), None) => Err(TelemetryError::config(
            &var,
            &value,
            "a client certificate needs its key: set OTEL_EXPORTER_OTLP_CLIENT_KEY too",
        )),
        (None, Some((var, value))) => Err(TelemetryError::config(
            &var,
            &value,
            "a client key needs its certificate: set OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE too",
        )),
    }
}

/// Whether the environment names the service, in which case it wins over
/// [`TelemetryBuilder::with_default_service_name`](super::TelemetryBuilder::with_default_service_name).
pub(super) fn service_name_from_env(env: &Env) -> bool {
    env("OTEL_SERVICE_NAME").is_some() || resource_attribute_from_env(env, "service.name")
}

/// Whether `OTEL_RESOURCE_ATTRIBUTES` sets `key`.
pub(super) fn resource_attribute_from_env(env: &Env, key: &str) -> bool {
    env("OTEL_RESOURCE_ATTRIBUTES").is_some_and(|attrs| {
        attrs
            .split(',')
            .filter_map(|kv| kv.split_once('='))
            .any(|(k, _)| k.trim() == key)
    })
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
