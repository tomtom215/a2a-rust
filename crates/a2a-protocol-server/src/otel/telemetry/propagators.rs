// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `OTEL_PROPAGATORS`: which propagators `Telemetry` installs as the global
//! one.
//!
//! Only `tracecontext` and `baggage` exist here (and `none`). A2A's own wire
//! handling does not use the global propagator — it reads and writes
//! `traceparent` itself — so the variable matters to the application's own
//! instrumentation. The variable is commonly set once for a fleet of services
//! in several languages, so a name this crate does not have (`b3`, `jaeger`,
//! `xray`, …) is reported, not refused: refusing would stop a process
//! starting over a value that is right for the next service.

use opentelemetry::propagation::{TextMapCompositePropagator, TextMapPropagator};
use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};

use super::config::Env;

/// Which of the two supported propagators to install, and what was asked for
/// that this crate does not have.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Propagators {
    pub(super) trace_context: bool,
    pub(super) baggage: bool,
    pub(super) unsupported: Vec<String>,
}

impl Propagators {
    const DEFAULT: Self = Self {
        trace_context: true,
        baggage: true,
        unsupported: Vec::new(),
    };

    /// What `OTEL_PROPAGATORS` asks for: `tracecontext,baggage` when unset;
    /// `none` for none; the supported names in a list; and, when the list
    /// names nothing supported, the default again — W3C propagation is what
    /// every A2A peer speaks — with the names left out reported.
    pub(super) fn from_env(env: &Env) -> Self {
        let Some(value) = env("OTEL_PROPAGATORS") else {
            return Self::DEFAULT;
        };
        let mut chosen = Self {
            trace_context: false,
            baggage: false,
            unsupported: Vec::new(),
        };
        for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match item.to_ascii_lowercase().as_str() {
                "none" => {
                    return Self {
                        trace_context: false,
                        baggage: false,
                        unsupported: Vec::new(),
                    };
                }
                "tracecontext" => chosen.trace_context = true,
                "baggage" => chosen.baggage = true,
                _ => chosen.unsupported.push(item.to_owned()),
            }
        }
        if !chosen.trace_context && !chosen.baggage {
            chosen.trace_context = true;
            chosen.baggage = true;
        }
        chosen
    }

    /// The propagator that installs these (an empty one for `none`).
    pub(super) fn composite(&self) -> TextMapCompositePropagator {
        let mut chosen: Vec<Box<dyn TextMapPropagator + Send + Sync>> = Vec::new();
        if self.trace_context {
            chosen.push(Box::new(TraceContextPropagator::new()));
        }
        if self.baggage {
            chosen.push(Box::new(BaggagePropagator::new()));
        }
        TextMapCompositePropagator::new(chosen)
    }

    /// The report for `unsupported`, if any.
    pub(super) fn notice(&self) -> Option<String> {
        (!self.unsupported.is_empty()).then(|| {
            format!(
                "OTEL_PROPAGATORS names {}, which this SDK does not have (it has `tracecontext`, \
                 `baggage` and `none`); {} installed",
                self.unsupported.join(", "),
                match (self.trace_context, self.baggage) {
                    (true, true) => "`tracecontext` and `baggage` are",
                    (true, false) => "`tracecontext` is",
                    (false, true) => "`baggage` is",
                    (false, false) => "none is",
                }
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(value: Option<&str>) -> Box<Env> {
        let map: HashMap<String, String> = value
            .map(|v| ("OTEL_PROPAGATORS".to_owned(), v.to_owned()))
            .into_iter()
            .collect();
        Box::new(move |k| map.get(k).cloned().filter(|v| !v.trim().is_empty()))
    }

    fn pair(p: &Propagators) -> (bool, bool) {
        (p.trace_context, p.baggage)
    }

    #[test]
    fn unset_and_empty_are_the_default() {
        assert_eq!(Propagators::from_env(&*env(None)), Propagators::DEFAULT);
        assert_eq!(
            Propagators::from_env(&*env(Some(" "))),
            Propagators::DEFAULT
        );
    }

    #[test]
    fn a_list_installs_exactly_what_it_names() {
        assert_eq!(
            pair(&Propagators::from_env(&*env(Some("baggage")))),
            (false, true)
        );
        assert_eq!(
            pair(&Propagators::from_env(&*env(Some("TraceContext")))),
            (true, false)
        );
        assert_eq!(
            pair(&Propagators::from_env(&*env(Some("tracecontext, baggage")))),
            (true, true)
        );
    }

    #[test]
    fn none_installs_nothing_and_beats_everything() {
        let p = Propagators::from_env(&*env(Some("none")));
        assert_eq!(pair(&p), (false, false));
        assert_eq!(
            pair(&Propagators::from_env(&*env(Some("baggage,none")))),
            (false, false)
        );
    }

    #[test]
    fn a_name_this_sdk_lacks_is_reported_and_the_rest_still_apply() {
        let p = Propagators::from_env(&*env(Some("tracecontext,b3,jaeger")));
        assert_eq!(pair(&p), (true, false));
        assert_eq!(p.unsupported, ["b3", "jaeger"]);
        let said = p.notice().expect("a notice");
        assert!(
            said.contains("b3, jaeger") && said.contains("`tracecontext` is installed"),
            "{said}"
        );
    }

    #[test]
    fn a_list_of_only_unsupported_names_falls_back_to_the_default_and_says_so() {
        let p = Propagators::from_env(&*env(Some("b3")));
        assert_eq!(pair(&p), (true, true));
        assert!(
            p.notice()
                .expect("a notice")
                .contains("`tracecontext` and `baggage` are installed")
        );
    }

    #[test]
    fn nothing_unsupported_means_no_notice() {
        assert_eq!(Propagators::from_env(&*env(Some("baggage"))).notice(), None);
    }
}
