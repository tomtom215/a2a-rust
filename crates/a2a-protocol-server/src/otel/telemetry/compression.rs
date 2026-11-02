// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Compression of OTLP export bodies.
//!
//! `OTEL_EXPORTER_OTLP_COMPRESSION` (and the per-signal forms) is read here,
//! not left to `opentelemetry-otlp`, for the reason `config.rs` gives for the
//! protocol: `opentelemetry-otlp` *warns and ignores* a value it was not built
//! to honour, and `Telemetry` has no subscriber yet when it builds the
//! exporters, so the warning went nowhere and the export went out
//! uncompressed. Measured 2026-10-26 against `hello-agent`: with
//! `OTEL_EXPORTER_OTLP_COMPRESSION=gzip` the collector received no
//! `Content-Encoding` and the same 6.6 KB body. A value this crate cannot act
//! on is noted, by name, in [`Telemetry::notices`](super::Telemetry::notices),
//! and the export is uncompressed: the specification has an SDK warn and ignore
//! it, and it was an error until 2026-11-01 (audit C-08).

use super::config::{Env, Notes, Signal};

/// How the body of an export request is compressed. Without one, it is not.
///
/// Only gzip is offered: every OTLP collector and the specification's
/// SDK-support list accept it, and the encoder is pure Rust. zstd needs a C
/// library, which this crate's dependency policy does not take on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OtlpCompression {
    /// gzip, as `Content-Encoding: gzip` over OTLP/HTTP and `grpc-encoding:
    /// gzip` over OTLP/gRPC.
    Gzip,
}

impl OtlpCompression {
    pub(super) const fn to_upstream(self) -> opentelemetry_otlp::Compression {
        match self {
            Self::Gzip => opentelemetry_otlp::Compression::Gzip,
        }
    }
}

/// The compression the environment names for `signal`: its own
/// `OTEL_EXPORTER_OTLP_{SIGNAL}_COMPRESSION`, else
/// `OTEL_EXPORTER_OTLP_COMPRESSION`. `none` and unset are both `None`.
///
/// Any other value is noted, naming the variable, and the export is not
/// compressed. `zstd` has its own reason, since the specification lists it and
/// a reader may expect it to work.
pub(super) fn from_env(env: &Env, signal: Signal, notes: &Notes) -> Option<OtlpCompression> {
    let (var, value) = signal.otlp_var(env, "COMPRESSION")?;
    match value.trim().to_ascii_lowercase().as_str() {
        "gzip" => Some(OtlpCompression::Gzip),
        "none" => None,
        "zstd" => {
            notes.ignored(
                &var,
                &value,
                "is not supported (zstd needs a C library); `gzip` and `none` are",
                "the export is not compressed",
            );
            None
        }
        _ => {
            notes.ignored(
                &var,
                &value,
                "is not `gzip` or `none`",
                "the export is not compressed",
            );
            None
        }
    }
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

    fn read(e: &Env, signal: Signal) -> (Option<OtlpCompression>, Vec<String>) {
        let notes = Notes::default();
        let got = from_env(e, signal, &notes);
        (got, notes.take())
    }

    #[test]
    fn unset_and_none_are_no_compression_and_say_nothing() {
        assert_eq!(read(&*env(&[]), Signal::Traces), (None, vec![]));
        let e = env(&[("OTEL_EXPORTER_OTLP_COMPRESSION", "none")]);
        assert_eq!(read(&*e, Signal::Traces), (None, vec![]));
    }

    #[test]
    fn gzip_is_read_in_any_case_and_with_spaces() {
        for v in ["gzip", "GZIP", " Gzip "] {
            let e = env(&[("OTEL_EXPORTER_OTLP_COMPRESSION", v)]);
            assert_eq!(
                read(&*e, Signal::Logs),
                (Some(OtlpCompression::Gzip), vec![]),
                "{v:?}"
            );
        }
    }

    #[test]
    fn the_signals_own_variable_wins_over_the_general_one_both_ways() {
        let e = env(&[
            ("OTEL_EXPORTER_OTLP_COMPRESSION", "gzip"),
            ("OTEL_EXPORTER_OTLP_METRICS_COMPRESSION", "none"),
        ]);
        assert_eq!(read(&*e, Signal::Metrics).0, None);
        assert_eq!(read(&*e, Signal::Traces).0, Some(OtlpCompression::Gzip));
        let e = env(&[("OTEL_EXPORTER_OTLP_LOGS_COMPRESSION", "gzip")]);
        assert_eq!(read(&*e, Signal::Logs).0, Some(OtlpCompression::Gzip));
        assert_eq!(read(&*e, Signal::Traces).0, None);
    }

    #[test]
    fn zstd_is_ignored_naming_the_variable_and_the_alternative() {
        let e = env(&[("OTEL_EXPORTER_OTLP_TRACES_COMPRESSION", "zstd")]);
        let (got, notes) = read(&*e, Signal::Traces);
        assert_eq!(got, None);
        let msg = &notes[0];
        assert!(
            msg.contains("OTEL_EXPORTER_OTLP_TRACES_COMPRESSION")
                && msg.contains("zstd")
                && msg.contains("gzip"),
            "{msg}"
        );
        // The fallback message for an unknown value carries the variable, the
        // value and "gzip" too; only the reason tells the two arms apart.
        assert!(msg.contains("is not supported"), "{msg}");
        assert!(msg.contains("the export is not compressed"), "{msg}");
    }

    #[test]
    fn an_unknown_value_is_ignored_naming_the_variable() {
        let e = env(&[("OTEL_EXPORTER_OTLP_COMPRESSION", "brotli")]);
        let (got, notes) = read(&*e, Signal::Logs);
        assert_eq!(got, None);
        let msg = &notes[0];
        assert!(
            msg.contains("OTEL_EXPORTER_OTLP_COMPRESSION") && msg.contains("brotli"),
            "{msg}"
        );
        assert!(msg.contains("is not `gzip` or `none`"), "{msg}");
    }

    #[test]
    fn an_empty_value_is_unset() {
        let e = env(&[("OTEL_EXPORTER_OTLP_COMPRESSION", "  ")]);
        assert_eq!(read(&*e, Signal::Traces), (None, vec![]));
    }
}
