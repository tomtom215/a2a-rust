// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`TelemetryError`]. Split out of `mod.rs` at the 500-line ratchet.

/// Why [`Telemetry`](super::Telemetry) could not be built, or lost telemetry.
#[derive(Debug)]
#[non_exhaustive]
pub enum TelemetryError {
    /// A variable, or a value given in code, this crate cannot act on.
    Config {
        /// The variable or setting.
        variable: String,
        /// Its value.
        value: String,
        /// What is wrong with it.
        reason: String,
    },
    /// `opentelemetry-otlp` refused to build an exporter.
    Exporter {
        /// `traces`, `metrics` or `logs`.
        signal: &'static str,
        /// Its error.
        source: opentelemetry_otlp::ExporterBuildError,
    },
    /// The TLS configuration could not be built.
    Tls(String),
    /// The export thread could not be started.
    Runtime(std::io::Error),
    /// A flush or shutdown failed, per signal.
    Export(Vec<String>),
}

impl TelemetryError {
    pub(super) fn config(variable: &str, value: &str, reason: impl Into<String>) -> Self {
        Self::Config {
            variable: variable.to_owned(),
            value: value.to_owned(),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config {
                variable,
                value,
                reason,
            } if value.is_empty() => {
                write!(f, "{variable}: {reason}")
            }
            Self::Config {
                variable,
                value,
                reason,
            } => write!(f, "{variable}={value:?}: {reason}"),
            Self::Exporter { signal, source } => write!(f, "{signal} exporter: {source}"),
            Self::Tls(reason) => write!(f, "OTLP TLS: {reason}"),
            Self::Runtime(e) => write!(f, "OTLP export thread: {e}"),
            Self::Export(failed) => write!(f, "OTLP export failed: {}", failed.join("; ")),
        }
    }
}

impl std::error::Error for TelemetryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Exporter { source, .. } => Some(source),
            Self::Runtime(e) => Some(e),
            _ => None,
        }
    }
}
