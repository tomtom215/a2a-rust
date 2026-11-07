// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The load signal (audit H7, ADR 0046): what a replica holds, for a
//! balancer, and the saturation readiness that may follow it.
//!
//! Readiness stays a dependency check. Load is reported beside it, at
//! `GET /load` and in the `endpoint-load-metrics` header, when the handler
//! is built with
//! [`with_load_reporting`](crate::RequestHandlerBuilder::with_load_reporting);
//! it drives readiness only when
//! [`HandlerLimits::with_saturation_readiness`](crate::handler::HandlerLimits::with_saturation_readiness)
//! is set. Every number is read without a lock on the queue map.

use std::sync::atomic::Ordering;

use super::RequestHandler;

/// The response header carrying the load, in the text form of an ORCA load
/// report: `TEXT application_utilization=0.250, named_metrics.running=3,
/// named_metrics.queued=0`.
pub const LOAD_HEADER: &str = "endpoint-load-metrics";

/// Whether a handler reports its load (ADR 0046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadReporting {
    /// No `/load`, no header: the default.
    Off,
    /// `/load` is served and responses carry [`LOAD_HEADER`].
    On,
}

/// What one replica holds now: see [`RequestHandler::load`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoadReport {
    /// Turns running: the live event queues, one per running turn.
    pub running: usize,
    /// Sends waiting in the admission gate (ADR 0045); `0` without one.
    pub queued: usize,
    /// The admission gate's capacity when there is a gate, else the
    /// concurrent-stream cap, else `None` (unbounded).
    pub capacity: Option<usize>,
    /// The replica is [draining](RequestHandler::begin_draining).
    pub draining: bool,
    /// A halt of every tenant is held ([`HaltScope::All`](crate::HaltScope::All)).
    pub halted: bool,
}

impl LoadReport {
    /// `(running + queued) / capacity` in thousandths, rounded down, which
    /// passes 1000 when sends are queued; `None` with no capacity or a
    /// capacity of 0.
    #[must_use]
    pub fn utilization_permille(&self) -> Option<u64> {
        let demand = wide(self.running) + wide(self.queued);
        let permille = (demand * 1000).checked_div(wide(self.capacity?))?;
        Some(u64::try_from(permille).unwrap_or(u64::MAX))
    }

    /// [`utilization_permille`](Self::utilization_permille) as a decimal
    /// with three places, as the header and `/load` write it.
    fn utilization_text(&self) -> Option<String> {
        self.utilization_permille()
            .map(|p| format!("{}.{:03}", p / 1000, p % 1000))
    }

    /// The `/load` body.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let utilization = self
            .utilization_text()
            .and_then(|text| text.parse::<serde_json::Number>().ok());
        serde_json::json!({
            "running": self.running,
            "queued": self.queued,
            "capacity": self.capacity,
            "utilization": utilization,
            "draining": self.draining,
            "halted": self.halted,
        })
    }

    /// The [`LOAD_HEADER`] value; with no capacity it has no utilization.
    #[must_use]
    pub fn header_value(&self) -> String {
        let named = format!(
            "named_metrics.running={}, named_metrics.queued={}",
            self.running, self.queued
        );
        self.utilization_text().map_or_else(
            || format!("TEXT {named}"),
            |u| format!("TEXT application_utilization={u}, {named}"),
        )
    }
}

/// `n` as a `u128`, which holds every `usize`.
fn wide(n: usize) -> u128 {
    u128::try_from(n).unwrap_or(u128::MAX)
}

/// Whether a replica is saturated after a probe that read `permille`, given
/// whether it was before: it becomes so at `unready_at` percent and stays so
/// until it is at or below `ready_at` percent. The band between the two is
/// the hysteresis.
fn next_saturated(was: bool, permille: u64, unready_at: u8, ready_at: u8) -> bool {
    if was {
        permille > u64::from(ready_at) * 10
    } else {
        permille >= u64::from(unready_at) * 10
    }
}

impl RequestHandler {
    /// What this replica holds now (ADR 0046). Cheap: atomics and two short
    /// locks, no store call and no lock on the queue map.
    #[must_use]
    pub fn load(&self) -> LoadReport {
        let admission = self.admission_load();
        LoadReport {
            running: self.event_queue_manager.live_queues(),
            queued: admission.map_or(0, |load| load.queued),
            capacity: admission
                .map(|load| load.capacity)
                .or_else(|| self.event_queue_manager.max_concurrent_queues()),
            draining: self.is_draining(),
            halted: self.is_halted_everywhere(),
        }
    }

    /// Whether readiness should answer `"saturated"`, updating the
    /// hysteresis state; always `false` unless saturation readiness is set
    /// and there is a capacity to measure against.
    pub(crate) fn saturated(&self) -> bool {
        let Some((unready_at, ready_at)) = self.limits.saturation_readiness else {
            return false;
        };
        let Some(permille) = self.load().utilization_permille() else {
            return false;
        };
        let was = self.saturated.load(Ordering::Acquire);
        let now = next_saturated(was, permille, unready_at, ready_at);
        self.saturated.store(now, Ordering::Release);
        now
    }

    /// Adds [`LOAD_HEADER`] to a response, when load reporting is on.
    pub(crate) fn stamp_load(&self, headers: &mut hyper::HeaderMap) {
        if !self.reports_load() {
            return;
        }
        if let Ok(value) = hyper::header::HeaderValue::try_from(self.load().header_value()) {
            headers.insert(LOAD_HEADER, value);
        }
    }

    /// Whether load reporting is on: `/load` is served and responses carry
    /// [`LOAD_HEADER`].
    #[must_use]
    pub const fn reports_load(&self) -> bool {
        matches!(self.load_reporting, LoadReporting::On)
    }
}

#[cfg(test)]
#[path = "load_tests.rs"]
mod tests;
