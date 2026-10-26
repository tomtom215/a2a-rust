// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which exporters a signal has: `OTEL_{TRACES,METRICS,LOGS}_EXPORTER`.
//!
//! The variable is a comma-separated list, so a signal can go to a collector
//! and be printed at once (`otlp,console`). `none` anywhere turns it off.
//! A value that names an exporter this crate does not have (`zipkin`, `jaeger`)
//! is refused rather than ignored: it would otherwise export nothing and say
//! nothing.

use super::TelemetryError;
use super::config::{Env, Signal};

/// The exporters one signal has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Selection {
    /// To an OTLP collector.
    pub(super) otlp: bool,
    /// Printed to standard output (`console`).
    pub(super) console: bool,
}

impl Selection {
    pub(super) const OFF: Self = Self {
        otlp: false,
        console: false,
    };

    /// Whether the signal is recorded at all.
    pub(super) const fn any(self) -> bool {
        self.otlp || self.console
    }
}

/// The exporters `signal` has: `OTEL_*_EXPORTER` when set, else OTLP when
/// `default_otlp` and no other.
///
/// # Errors
///
/// A value naming an exporter this crate does not have, naming the variable.
pub(super) fn from_env(
    env: &Env,
    signal: Signal,
    default_otlp: bool,
) -> Result<Selection, TelemetryError> {
    let var = signal.exporter_var();
    let Some(value) = env(var) else {
        return Ok(Selection {
            otlp: default_otlp,
            console: false,
        });
    };
    let mut chosen = Selection::OFF;
    for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match item.to_ascii_lowercase().as_str() {
            "none" => return Ok(Selection::OFF),
            "otlp" => chosen.otlp = true,
            // `logging` is the specification's deprecated name for `console`.
            "console" | "logging" => chosen.console = true,
            _ => {
                return Err(TelemetryError::config(
                    var,
                    &value,
                    "only `otlp`, `console` and `none` are supported",
                ));
            }
        }
    }
    Ok(chosen)
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

    const OTLP: Selection = Selection {
        otlp: true,
        console: false,
    };
    const CONSOLE: Selection = Selection {
        otlp: false,
        console: true,
    };

    #[test]
    fn exporter_variable_overrides_the_default_both_ways() {
        let e = env(&[
            ("OTEL_LOGS_EXPORTER", "none"),
            ("OTEL_METRICS_EXPORTER", "otlp"),
        ]);
        assert_eq!(from_env(&*e, Signal::Logs, true).unwrap(), Selection::OFF);
        assert_eq!(from_env(&*e, Signal::Metrics, false).unwrap(), OTLP);
        assert_eq!(from_env(&*e, Signal::Traces, true).unwrap(), OTLP);
        assert_eq!(
            from_env(&*e, Signal::Traces, false).unwrap(),
            Selection::OFF
        );
    }

    #[test]
    fn none_anywhere_in_the_list_wins() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "otlp, none")]);
        assert_eq!(from_env(&*e, Signal::Traces, true).unwrap(), Selection::OFF);
        let e = env(&[("OTEL_TRACES_EXPORTER", "none,console,otlp")]);
        assert_eq!(from_env(&*e, Signal::Traces, true).unwrap(), Selection::OFF);
    }

    #[test]
    fn console_alone_replaces_the_default_and_a_list_takes_both() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "console")]);
        assert_eq!(from_env(&*e, Signal::Traces, true).unwrap(), CONSOLE);
        let e = env(&[("OTEL_TRACES_EXPORTER", " otlp , console ,")]);
        assert_eq!(
            from_env(&*e, Signal::Traces, false).unwrap(),
            Selection {
                otlp: true,
                console: true
            }
        );
    }

    #[test]
    fn logging_is_the_deprecated_name_for_console() {
        let e = env(&[("OTEL_LOGS_EXPORTER", "LOGGING")]);
        assert_eq!(from_env(&*e, Signal::Logs, true).unwrap(), CONSOLE);
    }

    #[test]
    fn an_exporter_this_crate_lacks_is_refused_with_its_name() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "otlp,zipkin")]);
        let msg = from_env(&*e, Signal::Traces, true).unwrap_err().to_string();
        assert!(msg.contains("OTEL_TRACES_EXPORTER"), "{msg}");
        assert!(msg.contains("zipkin"), "{msg}");
    }

    #[test]
    fn an_empty_value_is_unset() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "  ")]);
        assert_eq!(from_env(&*e, Signal::Traces, true).unwrap(), OTLP);
        assert_eq!(
            from_env(&*e, Signal::Traces, false).unwrap(),
            Selection::OFF
        );
    }

    #[test]
    fn any_is_true_for_either_exporter() {
        assert!(OTLP.any() && CONSOLE.any() && !Selection::OFF.any());
    }
}
