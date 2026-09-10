// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A [`Metrics`] implementation that keeps what it is told, so an act can
//! read the SDK's own report of what happened and compare it with reality.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use a2a_protocol_server::metrics::Metrics;

/// Records every [`Metrics::on_push_delivery`] outcome and every
/// [`Metrics::on_persistence_error`], in order.
///
/// `BTreeMap` rather than `HashMap` so the summary prints in a stable order —
/// the transcript in the README is captured from a real run and should not
/// change between runs that did the same thing.
#[derive(Debug, Default)]
pub struct RecordingMetrics {
    push_outcomes: Mutex<Vec<String>>,
    persistence_errors: Mutex<Vec<(String, String)>>,
}

impl RecordingMetrics {
    /// Every push outcome reported so far, in the order it was reported.
    pub fn push_outcomes(&self) -> Vec<String> {
        self.push_outcomes.lock().expect("metrics lock").clone()
    }

    /// Outcome label → count.
    pub fn push_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for outcome in self.push_outcomes() {
            *counts.entry(outcome).or_insert(0) += 1;
        }
        counts
    }

    /// `(operation, error_kind)` for every persistence failure reported.
    ///
    /// Read by Act 1 and its unit test; a build without `sqlite` keeps it
    /// for the test alone.
    #[cfg_attr(not(feature = "sqlite"), allow(dead_code))]
    pub fn persistence_errors(&self) -> Vec<(String, String)> {
        self.persistence_errors
            .lock()
            .expect("metrics lock")
            .clone()
    }

    /// Waits until at least `n` push outcomes have been reported, or
    /// `budget` elapses. Returns how many were reported.
    ///
    /// Push delivery for a blocking `SendMessage` happens on the request
    /// path, but the reader should not have to know that: polling makes the
    /// assertion true or false on its own terms rather than on a scheduling
    /// accident.
    pub async fn wait_for_push_outcomes(&self, n: usize, budget: Duration) -> usize {
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            let seen = self.push_outcomes.lock().expect("metrics lock").len();
            if seen >= n || tokio::time::Instant::now() >= deadline {
                return seen;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// Renders `label=count` pairs, comma-separated.
pub fn render_counts(counts: &BTreeMap<String, usize>) -> String {
    if counts.is_empty() {
        return "(none)".to_owned();
    }
    counts
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Metrics for RecordingMetrics {
    fn on_push_delivery(&self, outcome: &str) {
        self.push_outcomes
            .lock()
            .expect("metrics lock")
            .push(outcome.to_owned());
    }

    fn on_persistence_error(&self, operation: &str, error_kind: &str) {
        self.persistence_errors
            .lock()
            .expect("metrics lock")
            .push((operation.to_owned(), error_kind.to_owned()));
    }
}
