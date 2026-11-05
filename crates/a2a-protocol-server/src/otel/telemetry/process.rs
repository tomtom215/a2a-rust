// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Process and Tokio runtime metrics (audit C-13).
//!
//! When metrics are exported, `Telemetry` reports the process's own health
//! under the semantic conventions' process metrics
//! (`docs/system/process-metrics.md`): `process.cpu.time` (s, `cpu.mode` =
//! `user` | `system`), `process.memory.usage` and `process.memory.virtual`
//! (By), `process.thread.count` and `process.unix.file_descriptor.count`.
//! They are read from `/proc/self` on Linux; on other systems they are not
//! reported (no series, rather than a wrong one), and nothing here needs a
//! new dependency or `unsafe`.
//!
//! The Tokio runtime the application runs on, when `Telemetry` is built
//! inside it or is given its handle, reports three of Tokio's stable
//! `RuntimeMetrics` (no `tokio_unstable`): `tokio.runtime.worker.count`,
//! `tokio.runtime.task.count` (alive tasks) and
//! `tokio.runtime.global_queue.depth`. The conventions name no Tokio metric,
//! so these are this crate's names, in their style.
//!
//! `TelemetryBuilder::without_process_metrics` turns both off.

use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_sdk::metrics::SdkMeterProvider;

use super::SCOPE;

// `MS_PER_TICK`, `tick_seconds`, `parse_stat` and `parse_status` read the
// Linux `/proc` formats and are called only by the Linux `read`. They are also
// built under `test`, so that the parsers' tests run on every platform;
// nowhere else would they be used, and an unused item fails `-D warnings`.

/// Milliseconds in one Linux `USER_HZ` clock tick, the unit of the CPU times
/// in `/proc/<pid>/stat`. `USER_HZ` is part of the kernel's user-space ABI
/// and is 100 on every architecture Rust's tier 1 and tier 2 Linux targets
/// run on (`proc(5)`: "measured in clock ticks (divide by
/// `sysconf(_SC_CLK_TCK)`)"); reading `sysconf` would take `unsafe` or a new
/// dependency for a constant.
#[cfg(any(target_os = "linux", test))]
const MS_PER_TICK: u64 = 10;

/// `ticks` of CPU time in seconds.
#[cfg(any(target_os = "linux", test))]
pub(super) const fn tick_seconds(ticks: u64) -> f64 {
    std::time::Duration::from_millis(ticks.saturating_mul(MS_PER_TICK)).as_secs_f64()
}

/// One reading of the process's own state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Snapshot {
    pub(super) cpu_user_seconds: f64,
    pub(super) cpu_system_seconds: f64,
    pub(super) resident_bytes: u64,
    pub(super) virtual_bytes: u64,
    pub(super) threads: u64,
    pub(super) open_fds: u64,
}

/// `utime` and `stime`, in clock ticks, from the text of `/proc/<pid>/stat`.
/// The command name (field 2) is in parentheses and may itself hold spaces
/// and parentheses, so the fields are counted from the last `)`.
#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_stat(text: &str) -> Option<(u64, u64)> {
    let rest = &text[text.rfind(')')? + 1..];
    let mut fields = rest.split_ascii_whitespace();
    // After the name: state (3), ppid, pgrp, session, tty_nr, tpgid, flags,
    // minflt, cminflt, majflt, cmajflt (13), utime (14), stime (15).
    let utime = fields.nth(11)?.parse().ok()?;
    let stime = fields.next()?.parse().ok()?;
    Some((utime, stime))
}

/// `VmRSS` and `VmSize` in bytes, and `Threads`, from `/proc/<pid>/status`.
#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_status(text: &str) -> Option<(u64, u64, u64)> {
    let mut rss = None;
    let mut size = None;
    let mut threads = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let number = || value.split_ascii_whitespace().next()?.parse::<u64>().ok();
        match key {
            "VmRSS" => rss = number().map(|kb| kb * 1024),
            "VmSize" => size = number().map(|kb| kb * 1024),
            "Threads" => threads = number(),
            _ => {}
        }
    }
    Some((rss?, size?, threads?))
}

/// The process's state now, or `None` where it cannot be read.
#[cfg(target_os = "linux")]
pub(super) fn read() -> Option<Snapshot> {
    let (utime, stime) = parse_stat(&std::fs::read_to_string("/proc/self/stat").ok()?)?;
    let (resident_bytes, virtual_bytes, threads) =
        parse_status(&std::fs::read_to_string("/proc/self/status").ok()?)?;
    let open_fds = u64::try_from(std::fs::read_dir("/proc/self/fd").ok()?.count()).ok()?;
    Some(Snapshot {
        cpu_user_seconds: tick_seconds(utime),
        cpu_system_seconds: tick_seconds(stime),
        resident_bytes,
        virtual_bytes,
        threads,
        open_fds,
    })
}

/// The process's state now: not read outside Linux.
#[cfg(not(target_os = "linux"))]
pub(super) const fn read() -> Option<Snapshot> {
    None
}

/// A process metric read from a [`Snapshot`]: name, unit, description, field.
type Gauge = (
    &'static str,
    &'static str,
    &'static str,
    fn(&Snapshot) -> u64,
);

/// A Tokio runtime metric: name, unit, description, reading.
type RuntimeCount = (
    &'static str,
    &'static str,
    &'static str,
    fn(&tokio::runtime::RuntimeMetrics) -> usize,
);

/// Registers the process metrics, and the runtime metrics of `runtime` when
/// one is given, on `provider`. The callbacks run at each collection.
pub(super) fn observe(provider: &SdkMeterProvider, runtime: Option<tokio::runtime::Handle>) {
    observe_with(provider, read, runtime);
}

/// [`observe`] with the reading injected, for the tests.
fn observe_with(
    provider: &SdkMeterProvider,
    read: fn() -> Option<Snapshot>,
    runtime: Option<tokio::runtime::Handle>,
) {
    let meter = provider.meter(SCOPE);
    let _cpu = meter
        .f64_observable_counter("process.cpu.time")
        .with_unit("s")
        .with_description("CPU time the process has used, by mode")
        .with_callback(move |o| {
            if let Some(s) = read() {
                o.observe(s.cpu_user_seconds, &[KeyValue::new("cpu.mode", "user")]);
                o.observe(s.cpu_system_seconds, &[KeyValue::new("cpu.mode", "system")]);
            }
        })
        .build();
    let gauges: [Gauge; 4] = [
        (
            "process.memory.usage",
            "By",
            "Resident memory of the process",
            |s| s.resident_bytes,
        ),
        (
            "process.memory.virtual",
            "By",
            "Virtual memory of the process",
            |s| s.virtual_bytes,
        ),
        (
            "process.thread.count",
            "{thread}",
            "Threads of the process",
            |s| s.threads,
        ),
        (
            "process.unix.file_descriptor.count",
            "{file_descriptor}",
            "File descriptors the process holds open",
            |s| s.open_fds,
        ),
    ];
    for (name, unit, description, field) in gauges {
        let _gauge = meter
            .i64_observable_up_down_counter(name)
            .with_unit(unit)
            .with_description(description)
            .with_callback(move |o| {
                if let Some(s) = read() {
                    o.observe(i64::try_from(field(&s)).unwrap_or(i64::MAX), &[]);
                }
            })
            .build();
    }
    if let Some(handle) = runtime {
        observe_runtime(&meter, &handle);
    }
}

/// Tokio's stable runtime metrics for the runtime `handle` drives.
fn observe_runtime(meter: &opentelemetry::metrics::Meter, handle: &tokio::runtime::Handle) {
    let counts: [RuntimeCount; 3] = [
        (
            "tokio.runtime.worker.count",
            "{thread}",
            "Worker threads of the Tokio runtime",
            tokio::runtime::RuntimeMetrics::num_workers,
        ),
        (
            "tokio.runtime.task.count",
            "{task}",
            "Tasks alive on the Tokio runtime",
            tokio::runtime::RuntimeMetrics::num_alive_tasks,
        ),
        (
            "tokio.runtime.global_queue.depth",
            "{task}",
            "Tasks waiting in the Tokio runtime's global queue",
            tokio::runtime::RuntimeMetrics::global_queue_depth,
        ),
    ];
    for (name, unit, description, value) in counts {
        let handle = handle.clone();
        let _count = meter
            .i64_observable_up_down_counter(name)
            .with_unit(unit)
            .with_description(description)
            .with_callback(move |o| {
                o.observe(
                    i64::try_from(value(&handle.metrics())).unwrap_or(i64::MAX),
                    &[],
                );
            })
            .build();
    }
}

#[cfg(test)]
mod tests;
