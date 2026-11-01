// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which exporters a signal has: `OTEL_{TRACES,METRICS,LOGS}_EXPORTER`.
//!
//! The variable is a comma-separated list, so a signal can go to a collector
//! and be printed at once (`otlp,console`). `none` anywhere turns it off.
//! A name this crate does not have (`zipkin`, `jaeger`) is dropped, with a
//! notice naming it; when nothing usable is left the variable counts as unset
//! and the default applies. Until 2026-11-01 such a value was refused, and the
//! process with it, over a variable a fleet sets for every SDK it runs (audit
//! C-08).

use super::config::{Env, Notes, Signal};

/// The exporters one signal has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Selection {
    /// To an OTLP collector.
    pub(super) otlp: bool,
    /// Printed to standard output (`console`).
    pub(super) console: bool,
    /// Served for a Prometheus to scrape (`prometheus`, metrics only).
    pub(super) prometheus: bool,
}

impl Selection {
    pub(super) const OFF: Self = Self {
        otlp: false,
        console: false,
        prometheus: false,
    };

    /// Whether the signal is recorded at all.
    pub(super) const fn any(self) -> bool {
        self.otlp || self.console || self.prometheus
    }

    /// Whether anything is sent to, or served for, a process outside this one:
    /// the cases that need the export runtime.
    pub(super) const fn needs_runtime(self) -> bool {
        self.otlp || self.prometheus
    }
}

/// The exporters `signal` has: `OTEL_*_EXPORTER` when set, else OTLP when
/// `default_otlp` and no other.
///
/// A name this crate cannot use is dropped and noted, once per variable. When
/// no name is left the variable is treated as unset, so the default applies.
pub(super) fn from_env(env: &Env, signal: Signal, default_otlp: bool, notes: &Notes) -> Selection {
    let var = signal.exporter_var();
    let default = Selection {
        otlp: default_otlp,
        ..Selection::OFF
    };
    let Some(value) = env(var) else {
        return default;
    };
    let mut chosen = Selection::OFF;
    let mut none = false;
    let mut dropped: Vec<String> = Vec::new();
    for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match item.to_ascii_lowercase().as_str() {
            "none" => none = true,
            "otlp" => chosen.otlp = true,
            // `logging` is the specification's deprecated name for `console`.
            "console" | "logging" => chosen.console = true,
            "prometheus" if signal == Signal::Metrics => {
                // The feature split is inside this one arm, not in two arms and
                // not in a helper that returns a constant: under
                // `--all-features` a `not(otel-prometheus)` arm is compiled out,
                // so deleting it builds and passes every test, and a helper
                // whose whole body is `Ok(true)` has a mutant (`Ok(true)`) that
                // nothing can tell from it.
                #[cfg(feature = "otel-prometheus")]
                {
                    chosen.prometheus = true;
                }
                #[cfg(not(feature = "otel-prometheus"))]
                {
                    dropped.push(format!(
                        "`{item}` needs the `otel-prometheus` feature, which is not compiled in"
                    ));
                }
            }
            "prometheus" => dropped.push(format!(
                "`{item}` exports metrics; it is not an exporter for this signal"
            )),
            _ => dropped.push(format!("`{item}` is not an exporter this SDK has")),
        }
    }
    if none {
        // `none` wins over everything else in the list; what else was named
        // does not matter.
        return Selection::OFF;
    }
    if dropped.is_empty() {
        return chosen;
    }
    let instead = if chosen.any() {
        "the others are used"
    } else {
        "nothing usable is left, so the default is used"
    };
    notes.ignored(
        var,
        &value,
        &format!(
            "{} (it has `otlp`, `console`, `prometheus` for metrics, and `none`)",
            dropped.join("; ")
        ),
        instead,
    );
    if chosen.any() { chosen } else { default }
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
        ..Selection::OFF
    };
    const CONSOLE: Selection = Selection {
        console: true,
        ..Selection::OFF
    };

    /// The selection, and the notes it left.
    fn read(e: &Env, signal: Signal, default_otlp: bool) -> (Selection, Vec<String>) {
        let notes = Notes::default();
        let got = from_env(e, signal, default_otlp, &notes);
        (got, notes.take())
    }

    #[test]
    fn exporter_variable_overrides_the_default_both_ways() {
        let e = env(&[
            ("OTEL_LOGS_EXPORTER", "none"),
            ("OTEL_METRICS_EXPORTER", "otlp"),
        ]);
        assert_eq!(read(&*e, Signal::Logs, true).0, Selection::OFF);
        assert_eq!(read(&*e, Signal::Metrics, false).0, OTLP);
        assert_eq!(read(&*e, Signal::Traces, true).0, OTLP);
        assert_eq!(read(&*e, Signal::Traces, false).0, Selection::OFF);
    }

    #[test]
    fn none_anywhere_in_the_list_wins() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "otlp, none")]);
        assert_eq!(read(&*e, Signal::Traces, true).0, Selection::OFF);
        let e = env(&[("OTEL_TRACES_EXPORTER", "none,console,otlp")]);
        assert_eq!(read(&*e, Signal::Traces, true).0, Selection::OFF);
    }

    #[test]
    fn console_alone_replaces_the_default_and_a_list_takes_both() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "console")]);
        assert_eq!(read(&*e, Signal::Traces, true).0, CONSOLE);
        let e = env(&[("OTEL_TRACES_EXPORTER", " otlp , console ,")]);
        assert_eq!(
            read(&*e, Signal::Traces, false).0,
            Selection {
                otlp: true,
                console: true,
                ..Selection::OFF
            }
        );
    }

    #[test]
    fn logging_is_the_deprecated_name_for_console() {
        let e = env(&[("OTEL_LOGS_EXPORTER", "LOGGING")]);
        assert_eq!(read(&*e, Signal::Logs, true).0, CONSOLE);
    }

    /// A name this crate lacks is dropped with a notice that names it, and the
    /// rest of the list is used (audit C-08).
    #[test]
    fn an_exporter_this_crate_lacks_is_dropped_and_named() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "console,zipkin")]);
        let (got, notes) = read(&*e, Signal::Traces, true);
        assert_eq!(got, CONSOLE, "the rest of the list is used");
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("OTEL_TRACES_EXPORTER"), "{notes:?}");
        assert!(
            notes[0].contains("`zipkin` is not an exporter"),
            "{notes:?}"
        );
        assert!(notes[0].contains("the others are used"), "{notes:?}");
    }

    /// With nothing usable left the variable is as good as unset: the default
    /// applies, both ways, and the notice says so.
    #[test]
    fn a_list_with_nothing_usable_in_it_leaves_the_default() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "zipkin, jaeger")]);
        let (on, notes) = read(&*e, Signal::Traces, true);
        assert_eq!(on, OTLP);
        assert!(
            notes[0].contains("`zipkin`") && notes[0].contains("`jaeger`"),
            "{notes:?}"
        );
        assert!(notes[0].contains("the default is used"), "{notes:?}");
        assert_eq!(read(&*e, Signal::Traces, false).0, Selection::OFF);
    }

    /// `none` decides the signal, so a name beside it is not worth a notice.
    #[test]
    fn a_name_beside_none_is_not_reported() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "zipkin,none")]);
        let (got, notes) = read(&*e, Signal::Traces, true);
        assert_eq!(got, Selection::OFF);
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_good_value_leaves_no_notice() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "otlp,console")]);
        assert_eq!(read(&*e, Signal::Traces, false).1, Vec::<String>::new());
    }

    #[test]
    fn an_empty_value_is_unset() {
        let e = env(&[("OTEL_TRACES_EXPORTER", "  ")]);
        assert_eq!(read(&*e, Signal::Traces, true), (OTLP, vec![]));
        assert_eq!(read(&*e, Signal::Traces, false), (Selection::OFF, vec![]));
    }

    #[test]
    fn any_is_true_for_each_exporter_and_only_a_remote_one_needs_the_runtime() {
        let prometheus = Selection {
            prometheus: true,
            ..Selection::OFF
        };
        assert!(OTLP.any() && CONSOLE.any() && prometheus.any() && !Selection::OFF.any());
        assert!(OTLP.needs_runtime() && prometheus.needs_runtime());
        assert!(!CONSOLE.needs_runtime() && !Selection::OFF.needs_runtime());
    }

    #[test]
    fn prometheus_is_for_metrics_alone() {
        for signal in [Signal::Traces, Signal::Logs] {
            let e = env(&[
                ("OTEL_TRACES_EXPORTER", "prometheus"),
                ("OTEL_LOGS_EXPORTER", "prometheus"),
            ]);
            let (got, notes) = read(&*e, signal, true);
            assert_eq!(got, OTLP, "dropped, so the default");
            // The arm's own words: the fallback for an unknown exporter also
            // names `prometheus` and `metrics`, in its list of what is supported.
            assert!(
                notes[0].contains("not an exporter for this signal"),
                "{notes:?}"
            );
        }
    }

    #[cfg(not(feature = "otel-prometheus"))]
    #[test]
    fn prometheus_without_its_feature_is_dropped_naming_the_feature() {
        let e = env(&[("OTEL_METRICS_EXPORTER", "prometheus")]);
        let (got, notes) = read(&*e, Signal::Metrics, true);
        assert_eq!(got, OTLP, "nothing usable left: the default");
        assert!(
            notes[0].contains("OTEL_METRICS_EXPORTER") && notes[0].contains("otel-prometheus"),
            "{notes:?}"
        );
    }

    #[cfg(feature = "otel-prometheus")]
    #[test]
    fn prometheus_with_its_feature_is_chosen_alone_or_beside_otlp() {
        let only = env(&[("OTEL_METRICS_EXPORTER", "prometheus")]);
        let (got, notes) = read(&*only, Signal::Metrics, true);
        assert!(got.prometheus && !got.otlp && !got.console);
        assert!(notes.is_empty(), "{notes:?}");
        let both = env(&[("OTEL_METRICS_EXPORTER", "otlp,prometheus")]);
        let got = read(&*both, Signal::Metrics, false).0;
        assert!(got.prometheus && got.otlp);
    }
}
