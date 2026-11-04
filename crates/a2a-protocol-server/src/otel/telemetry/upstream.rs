// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Variables the upstream crates read for themselves, checked here so a value
//! they ignore is said out loud (audit NEW-c-3).
//!
//! `opentelemetry_sdk` 0.33.1 reads `OTEL_TRACES_SAMPLER` and
//! `OTEL_TRACES_SAMPLER_ARG` when the tracer provider is built
//! (`trace/config.rs`), and `opentelemetry-otlp` 0.33.1 reads
//! `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE` when the metric exporter
//! is built (`metric.rs`, `resolve_temporality`). Each warns about a value it
//! cannot use through `otel_warn!` and falls back — as the specification asks
//! ("Environment Variable Specification", *Parsing empty value* and *Enum
//! value*: an unrecognised value MUST generate a warning and be ignored). But
//! both run inside `TelemetryBuilder::build`, before
//! `Telemetry::install_subscriber` has installed any subscriber, so the
//! warning reached no one and the setting was silently replaced. The same
//! rules are applied here, to the same values, and what does not parse goes
//! to [`Telemetry::notices`](super::Telemetry::notices), which
//! `install_subscriber` prints. Nothing here changes what the upstream crates
//! do; the notes say what they do.

use super::config::{Env, Notes};

/// The samplers `opentelemetry_sdk` 0.33.1 builds from `OTEL_TRACES_SAMPLER`,
/// matched as it matches them: exactly, case and all.
const SAMPLERS: [&str; 6] = [
    "always_on",
    "always_off",
    "traceidratio",
    "parentbased_always_on",
    "parentbased_always_off",
    "parentbased_traceidratio",
];

/// Sampler names the specification lists that this SDK knows of but does not
/// implement; it falls back as for an unknown one.
const UNIMPLEMENTED_SAMPLERS: [&str; 3] = ["jaeger_remote", "parentbased_jaeger_remote", "xray"];

/// What the SDK does with a sampler it cannot use.
const SAMPLER_FALLBACK: &str = "`parentbased_always_on` is used";

/// Notes on `OTEL_TRACES_SAMPLER` and `OTEL_TRACES_SAMPLER_ARG`, for a process
/// whose tracer provider is built.
pub(super) fn note_sampler(env: &Env, notes: &Notes) {
    const VAR: &str = "OTEL_TRACES_SAMPLER";
    let Some(sampler) = env(VAR) else {
        return;
    };
    if UNIMPLEMENTED_SAMPLERS.contains(&sampler.as_str()) {
        notes.ignored(
            VAR,
            &sampler,
            "names a sampler this SDK does not implement",
            SAMPLER_FALLBACK,
        );
        return;
    }
    if !SAMPLERS.contains(&sampler.as_str()) {
        notes.ignored(
            VAR,
            &sampler,
            &format!(
                "is not one of {} (matched exactly, in lower case)",
                SAMPLERS.join(", ")
            ),
            SAMPLER_FALLBACK,
        );
        return;
    }
    if sampler.ends_with("traceidratio") {
        note_sampler_arg(env, notes);
    }
}

/// A ratio sampler reads its probability from `OTEL_TRACES_SAMPLER_ARG`; the
/// SDK samples everything when it is missing or not a number.
fn note_sampler_arg(env: &Env, notes: &Notes) {
    const ARG: &str = "OTEL_TRACES_SAMPLER_ARG";
    match env(ARG) {
        None => notes.ignored(
            ARG,
            "",
            "is needed by a ratio sampler and is not set",
            "the ratio 1.0 (every trace) is used",
        ),
        Some(arg) if arg.parse::<f64>().is_err() => notes.ignored(
            ARG,
            &arg,
            "is not a number",
            "the ratio 1.0 (every trace) is used",
        ),
        Some(arg) if !(0.0..=1.0).contains(&arg.parse::<f64>().unwrap_or(1.0)) => notes.ignored(
            ARG,
            &arg,
            "is outside 0.0 to 1.0, the range the specification gives a probability",
            "it is passed to the SDK as it is",
        ),
        Some(_) => {}
    }
}

/// A note on `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE`, for a process
/// that exports metrics over OTLP.
pub(super) fn note_temporality(env: &Env, notes: &Notes) {
    const VAR: &str = "OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE";
    if let Some(value) = env(VAR)
        && !["cumulative", "delta", "lowmemory"].contains(&value.to_lowercase().as_str())
    {
        notes.ignored(
            VAR,
            &value,
            "is not `cumulative`, `delta` or `lowmemory`",
            "`cumulative` is used",
        );
    }
}

/// A note on `OTEL_METRIC_EXPORT_INTERVAL`, which `PeriodicReader` reads and
/// this crate's startup line shows, when it is not a number of milliseconds.
pub(super) fn note_metric_interval(env: &Env, notes: &Notes) {
    const VAR: &str = "OTEL_METRIC_EXPORT_INTERVAL";
    if let Some(value) = env(VAR)
        && value.trim().parse::<u64>().is_err()
    {
        notes.ignored(
            VAR,
            &value,
            "is not a number of milliseconds",
            "60000 is used",
        );
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
        Box::new(move |k| map.get(k).cloned())
    }

    fn sampler_notes(pairs: &[(&str, &str)]) -> Vec<String> {
        let notes = Notes::default();
        note_sampler(&*env(pairs), &notes);
        notes.take()
    }

    /// Spec: Environment Variable Specification, "Enum value": an
    /// unrecognised value MUST generate a warning and be ignored.
    #[test]
    fn an_unknown_sampler_is_noted_with_what_the_sdk_uses_instead() {
        assert_eq!(
            sampler_notes(&[("OTEL_TRACES_SAMPLER", "traceidratio-typo")]),
            [format!(
                "OTEL_TRACES_SAMPLER=\"traceidratio-typo\" is not one of {} (matched exactly, \
                 in lower case); `parentbased_always_on` is used",
                SAMPLERS.join(", ")
            )]
        );
        // The SDK matches exactly: an upper-case name is unknown to it too.
        assert_eq!(
            sampler_notes(&[("OTEL_TRACES_SAMPLER", "Always_On")]).len(),
            1
        );
    }

    #[test]
    fn a_sampler_the_sdk_knows_but_lacks_is_noted_as_such() {
        for name in UNIMPLEMENTED_SAMPLERS {
            assert_eq!(
                sampler_notes(&[("OTEL_TRACES_SAMPLER", name)]),
                [format!(
                    "OTEL_TRACES_SAMPLER={name:?} names a sampler this SDK does not implement; \
                     `parentbased_always_on` is used"
                )]
            );
        }
    }

    #[test]
    fn every_sampler_the_sdk_builds_passes_silently() {
        for name in SAMPLERS {
            let got = sampler_notes(&[
                ("OTEL_TRACES_SAMPLER", name),
                ("OTEL_TRACES_SAMPLER_ARG", "0.25"),
            ]);
            assert!(got.is_empty(), "{name}: {got:?}");
        }
        assert_eq!(sampler_notes(&[]), Vec::<String>::new());
    }

    #[test]
    fn a_ratio_sampler_without_a_usable_argument_is_noted() {
        for sampler in ["traceidratio", "parentbased_traceidratio"] {
            assert_eq!(
                sampler_notes(&[("OTEL_TRACES_SAMPLER", sampler)]),
                [
                    "OTEL_TRACES_SAMPLER_ARG=\"\" is needed by a ratio sampler and is not set; \
                  the ratio 1.0 (every trace) is used"
                ]
            );
            assert_eq!(
                sampler_notes(&[
                    ("OTEL_TRACES_SAMPLER", sampler),
                    ("OTEL_TRACES_SAMPLER_ARG", "half")
                ]),
                [
                    "OTEL_TRACES_SAMPLER_ARG=\"half\" is not a number; the ratio 1.0 (every trace) \
                  is used"
                ]
            );
            assert_eq!(
                sampler_notes(&[
                    ("OTEL_TRACES_SAMPLER", sampler),
                    ("OTEL_TRACES_SAMPLER_ARG", "1.5")
                ]),
                [
                    "OTEL_TRACES_SAMPLER_ARG=\"1.5\" is outside 0.0 to 1.0, the range the \
                  specification gives a probability; it is passed to the SDK as it is"
                ]
            );
        }
        // The bounds themselves are in range.
        for arg in ["0", "1", "0.0", "1.0"] {
            assert_eq!(
                sampler_notes(&[
                    ("OTEL_TRACES_SAMPLER", "traceidratio"),
                    ("OTEL_TRACES_SAMPLER_ARG", arg)
                ]),
                Vec::<String>::new(),
                "{arg}"
            );
        }
        // A sampler that takes no argument ignores one.
        assert_eq!(
            sampler_notes(&[
                ("OTEL_TRACES_SAMPLER", "always_on"),
                ("OTEL_TRACES_SAMPLER_ARG", "half")
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_unknown_temporality_is_noted_and_known_ones_in_any_case_are_not() {
        const VAR: &str = "OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE";
        let notes = Notes::default();
        note_temporality(&*env(&[(VAR, "Delta-typo")]), &notes);
        assert_eq!(
            notes.take(),
            [format!(
                "{VAR}=\"Delta-typo\" is not `cumulative`, `delta` or `lowmemory`; \
                 `cumulative` is used"
            )]
        );
        for ok in ["cumulative", "Delta", "LOWMEMORY"] {
            note_temporality(&*env(&[(VAR, ok)]), &notes);
            assert!(notes.take().is_empty(), "{ok}");
        }
        note_temporality(&*env(&[]), &notes);
        assert_eq!(notes.take(), Vec::<String>::new());
    }

    #[test]
    fn a_metric_interval_that_is_not_a_number_is_noted() {
        let notes = Notes::default();
        note_metric_interval(&*env(&[("OTEL_METRIC_EXPORT_INTERVAL", "10s")]), &notes);
        assert_eq!(
            notes.take(),
            ["OTEL_METRIC_EXPORT_INTERVAL=\"10s\" is not a number of milliseconds; 60000 is used"]
        );
        note_metric_interval(&*env(&[("OTEL_METRIC_EXPORT_INTERVAL", " 5000 ")]), &notes);
        assert_eq!(notes.take(), Vec::<String>::new());
        note_metric_interval(&*env(&[]), &notes);
        assert_eq!(notes.take(), Vec::<String>::new());
    }
}
