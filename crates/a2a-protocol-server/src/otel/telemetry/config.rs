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

use std::time::Duration;

use super::TelemetryError;

/// Where variables come from: the process environment, or a test's table.
pub(super) type Env = dyn Fn(&str) -> Option<String> + Send + Sync;

/// The process environment, with an empty value read as unset — as the
/// specification asks ("an empty value MUST be treated as if it were not
/// set").
pub(super) fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
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
    const fn exporter_var(self) -> &'static str {
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

    /// The signal-specific variable, falling back to the general one.
    fn otlp_var(self, env: &Env, suffix: &str) -> Option<(String, String)> {
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
}

/// `OTEL_SDK_DISABLED=true` turns every signal off.
pub(super) fn sdk_disabled(env: &Env) -> bool {
    env("OTEL_SDK_DISABLED").is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}

/// Whether `signal` is exported: `OTEL_*_EXPORTER` when set, else `default`.
///
/// The variable is a comma-separated list. `none` turns the signal off;
/// `otlp` turns it on; anything else (`console`, `prometheus`, `zipkin`, …)
/// is refused rather than ignored, because a value that names an exporter
/// this crate does not have would otherwise export nothing and say nothing.
pub(super) fn exporter_enabled(
    env: &Env,
    signal: Signal,
    default: bool,
) -> Result<bool, TelemetryError> {
    let var = signal.exporter_var();
    let Some(value) = env(var) else {
        return Ok(default);
    };
    let mut otlp = false;
    for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match item.to_ascii_lowercase().as_str() {
            "none" => return Ok(false),
            "otlp" => otlp = true,
            _ => {
                return Err(TelemetryError::config(
                    var,
                    &value,
                    "only `otlp` and `none` are supported",
                ));
            }
        }
    }
    Ok(otlp)
}

/// The transport for `signal`: `explicit` when given, else
/// `OTEL_EXPORTER_OTLP_{SIGNAL}_PROTOCOL` or `OTEL_EXPORTER_OTLP_PROTOCOL`,
/// else HTTP/protobuf, the specification's default.
pub(super) fn protocol(
    env: &Env,
    signal: Signal,
    explicit: Option<OtlpProtocol>,
) -> Result<OtlpProtocol, TelemetryError> {
    if let Some(p) = explicit {
        return Ok(p);
    }
    let Some((var, value)) = signal.otlp_var(env, "PROTOCOL") else {
        return Ok(OtlpProtocol::HttpProtobuf);
    };
    match value.trim() {
        "grpc" => Ok(OtlpProtocol::Grpc),
        "http/protobuf" => Ok(OtlpProtocol::HttpProtobuf),
        "http/json" => Err(TelemetryError::config(
            &var,
            &value,
            "OTLP/HTTP with JSON bodies is not compiled in; use `http/protobuf`",
        )),
        _ => Err(TelemetryError::config(
            &var,
            &value,
            "expected `grpc` or `http/protobuf`",
        )),
    }
}

/// The export timeout for `signal`, in milliseconds per the specification;
/// 10 s when unset.
pub(super) fn timeout(env: &Env, signal: Signal) -> Result<Duration, TelemetryError> {
    match signal.otlp_var(env, "TIMEOUT") {
        None => Ok(Duration::from_secs(10)),
        Some((var, value)) => value
            .trim()
            .parse::<u64>()
            .map(Duration::from_millis)
            .map_err(|_| TelemetryError::config(&var, &value, "expected milliseconds")),
    }
}

/// The PEM file of extra CA certificates to trust for `signal`, if any:
/// `OTEL_EXPORTER_OTLP_{SIGNAL}_CERTIFICATE` or
/// `OTEL_EXPORTER_OTLP_CERTIFICATE`.
pub(super) fn certificate_file(env: &Env, signal: Signal) -> Option<(String, String)> {
    signal.otlp_var(env, "CERTIFICATE")
}

/// Whether the environment names the service, in which case it wins over
/// [`TelemetryBuilder::with_default_service_name`](super::TelemetryBuilder::with_default_service_name).
pub(super) fn service_name_from_env(env: &Env) -> bool {
    env("OTEL_SERVICE_NAME").is_some()
        || env("OTEL_RESOURCE_ATTRIBUTES").is_some_and(|attrs| {
            attrs
                .split(',')
                .filter_map(|kv| kv.split_once('='))
                .any(|(k, _)| k.trim() == "service.name")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> Box<Env> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Box::new(move |k| map.get(k).cloned().filter(|v| !v.trim().is_empty()))
    }

    #[test]
    fn exporter_variable_overrides_the_default_both_ways() {
        let e = env(&[
            ("OTEL_LOGS_EXPORTER", "none"),
            ("OTEL_METRICS_EXPORTER", "otlp"),
        ]);
        assert!(!exporter_enabled(&*e, Signal::Logs, true).unwrap());
        assert!(exporter_enabled(&*e, Signal::Metrics, false).unwrap());
        assert!(exporter_enabled(&*e, Signal::Traces, true).unwrap());
        assert!(!exporter_enabled(&*e, Signal::Traces, false).unwrap());
    }

    #[test]
    fn none_anywhere_in_the_list_wins() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "otlp, none")]);
        assert!(!exporter_enabled(&*e, Signal::Traces, true).unwrap());
    }

    #[test]
    fn an_exporter_this_crate_lacks_is_refused_with_its_name() {
        let e = env(&[("OTEL_METRICS_EXPORTER", "prometheus")]);
        let err = exporter_enabled(&*e, Signal::Metrics, true).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("OTEL_METRICS_EXPORTER"), "{msg}");
        assert!(msg.contains("prometheus"), "{msg}");
    }

    #[test]
    fn sdk_disabled_reads_only_true() {
        assert!(sdk_disabled(&*env(&[("OTEL_SDK_DISABLED", "TRUE")])));
        assert!(!sdk_disabled(&*env(&[("OTEL_SDK_DISABLED", "false")])));
        assert!(!sdk_disabled(&*env(&[("OTEL_SDK_DISABLED", "1")])));
        assert!(!sdk_disabled(&*env(&[])));
    }

    #[test]
    fn protocol_prefers_explicit_then_signal_then_general_then_http() {
        let e = env(&[
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
            ("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "http/protobuf"),
        ]);
        assert_eq!(
            protocol(&*e, Signal::Traces, None).unwrap(),
            OtlpProtocol::HttpProtobuf
        );
        assert_eq!(
            protocol(&*e, Signal::Metrics, None).unwrap(),
            OtlpProtocol::Grpc
        );
        assert_eq!(
            protocol(&*e, Signal::Metrics, Some(OtlpProtocol::HttpProtobuf)).unwrap(),
            OtlpProtocol::HttpProtobuf
        );
        assert_eq!(
            protocol(&*env(&[]), Signal::Logs, None).unwrap(),
            OtlpProtocol::HttpProtobuf
        );
    }

    #[test]
    fn protocol_refuses_json_and_garbage_naming_the_variable() {
        let e = env(&[("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json")]);
        let msg = protocol(&*e, Signal::Traces, None).unwrap_err().to_string();
        assert!(
            msg.contains("OTEL_EXPORTER_OTLP_PROTOCOL") && msg.contains("http/json"),
            "{msg}"
        );
        let e = env(&[("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL", "udp")]);
        let msg = protocol(&*e, Signal::Logs, None).unwrap_err().to_string();
        assert!(msg.contains("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL"), "{msg}");
    }

    #[test]
    fn timeout_is_milliseconds_with_a_ten_second_default() {
        assert_eq!(
            timeout(&*env(&[]), Signal::Traces).unwrap(),
            Duration::from_secs(10)
        );
        let e = env(&[
            ("OTEL_EXPORTER_OTLP_TIMEOUT", "2500"),
            ("OTEL_EXPORTER_OTLP_LOGS_TIMEOUT", "100"),
        ]);
        assert_eq!(
            timeout(&*e, Signal::Traces).unwrap(),
            Duration::from_millis(2500)
        );
        assert_eq!(
            timeout(&*e, Signal::Logs).unwrap(),
            Duration::from_millis(100)
        );
        assert!(
            timeout(
                &*env(&[("OTEL_EXPORTER_OTLP_TIMEOUT", "5s")]),
                Signal::Traces
            )
            .is_err()
        );
    }

    #[test]
    fn service_name_counts_from_either_variable_and_not_from_others() {
        assert!(service_name_from_env(&*env(&[("OTEL_SERVICE_NAME", "a")])));
        assert!(service_name_from_env(&*env(&[(
            "OTEL_RESOURCE_ATTRIBUTES",
            "deployment.environment.name=prod, service.name=b"
        )])));
        assert!(!service_name_from_env(&*env(&[(
            "OTEL_RESOURCE_ATTRIBUTES",
            "service.namespace=x"
        )])));
        assert!(!service_name_from_env(&*env(&[])));
    }

    #[test]
    fn an_empty_value_is_unset() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "  "), ("OTEL_SERVICE_NAME", "")]);
        assert!(exporter_enabled(&*e, Signal::Traces, true).unwrap());
        assert!(!service_name_from_env(&*e));
    }
}
