// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Process and runtime metrics (semantic conventions
//! `docs/system/process-metrics.md`; Tokio's stable `RuntimeMetrics`).

use std::sync::Arc;

use opentelemetry_sdk::metrics::ManualReader;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};

use super::*;

/// A `/proc/<pid>/stat` line whose command name holds spaces and both
/// parentheses, as `proc(5)` allows.
const STAT: &str = "4242 (my (odd) agent) S 1 4242 4242 0 -1 4194560 812 0 3 0 250 75 0 0 20 0 9 0 \
                    123 456 789\n";

const STATUS: &str = "Name:\tagent\nState:\tS (sleeping)\nVmSize:\t  204800 kB\nVmRSS:\t    4096 kB\n\
                      Threads:\t9\nSigQ:\t0/63\n";

#[test]
fn the_cpu_times_are_counted_from_after_the_last_parenthesis() {
    assert_eq!(parse_stat(STAT), Some((250, 75)));
}

#[test]
fn a_stat_line_it_cannot_read_is_none() {
    assert_eq!(
        parse_stat("4242 agent S 1 2 3"),
        None,
        "no name in parentheses"
    );
    assert_eq!(
        parse_stat("4242 (a) S 1 2 3 4 5 6 7 8 9 10"),
        None,
        "too short"
    );
    assert_eq!(
        parse_stat("4242 (a) S 1 2 3 4 5 6 7 8 9 10 x 75"),
        None,
        "utime not a number"
    );
    assert_eq!(
        parse_stat("4242 (a) S 1 2 3 4 5 6 7 8 9 10 250"),
        None,
        "no stime"
    );
}

#[test]
fn memory_is_read_in_bytes_and_threads_as_a_count() {
    assert_eq!(parse_status(STATUS), Some((4096 * 1024, 204_800 * 1024, 9)));
}

#[test]
fn a_status_missing_a_field_is_none() {
    for missing in ["VmRSS", "VmSize", "Threads"] {
        let text = STATUS
            .lines()
            .filter(|l| !l.starts_with(missing))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(parse_status(&text), None, "{missing}");
    }
}

#[test]
fn ticks_are_hundredths_of_a_second() {
    assert!((tick_seconds(250) - 2.5).abs() < f64::EPSILON);
    assert!((tick_seconds(1) - 0.01).abs() < f64::EPSILON);
    assert!(tick_seconds(u64::MAX) > 0.0, "saturates, does not overflow");
}

#[cfg(target_os = "linux")]
#[test]
fn this_process_is_read_on_linux() {
    let s = read().expect("/proc/self is readable");
    assert!(s.threads >= 1, "{s:?}");
    assert!(s.resident_bytes > 0, "{s:?}");
    assert!(s.virtual_bytes >= s.resident_bytes, "{s:?}");
    assert!(s.open_fds >= 1, "{s:?}");
    assert!(
        s.cpu_user_seconds >= 0.0 && s.cpu_system_seconds >= 0.0,
        "{s:?}"
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn nothing_is_read_outside_linux() {
    assert_eq!(read(), None);
}

/// Each metric's points, as (attributes, value), sorted.
fn points(reader: &Arc<ManualReader>, name: &str) -> Vec<(String, f64)> {
    let exported = crate::otel::test_reader::collect(reader);
    let mut out = Vec::new();
    for metric in exported
        .scope_metrics()
        .filter(|s| s.scope().name() == SCOPE)
        .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
        .filter(|m| m.name() == name)
    {
        let attrs = |a: &mut dyn Iterator<Item = &KeyValue>| {
            a.map(|kv| format!("{}={}", kv.key, kv.value))
                .collect::<Vec<_>>()
                .join(",")
        };
        match metric.data() {
            AggregatedMetrics::F64(MetricData::Sum(sum)) => {
                for p in sum.data_points() {
                    out.push((attrs(&mut p.attributes()), p.value()));
                }
            }
            AggregatedMetrics::I64(MetricData::Sum(sum)) => {
                for p in sum.data_points() {
                    out.push((
                        attrs(&mut p.attributes()),
                        f64::from(i32::try_from(p.value()).unwrap()),
                    ));
                }
            }
            other => panic!("{name}: {other:?}"),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn reader_and_provider() -> (Arc<ManualReader>, SdkMeterProvider) {
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(crate::otel::test_reader::SharedReader(Arc::clone(&reader)))
        .build();
    (reader, provider)
}

/// A fixed reading. A closure, so it coerces to the `fn` the callbacks take.
const FIXED: fn() -> Option<Snapshot> = || {
    Some(Snapshot {
        cpu_user_seconds: 2.5,
        cpu_system_seconds: 0.75,
        resident_bytes: 4_194_304,
        virtual_bytes: 209_715_200,
        threads: 9,
        open_fds: 12,
    })
};

/// No reading, as outside Linux.
const ABSENT: fn() -> Option<Snapshot> = || None;

#[test]
fn each_process_metric_reports_the_reading_under_its_conventions_name() {
    let (reader, provider) = reader_and_provider();
    observe_with(&provider, FIXED, None);
    let none = String::new;
    assert_eq!(
        points(&reader, "process.cpu.time"),
        [
            ("cpu.mode=system".to_owned(), 0.75),
            ("cpu.mode=user".to_owned(), 2.5)
        ]
    );
    assert_eq!(
        points(&reader, "process.memory.usage"),
        [(none(), 4_194_304.0)]
    );
    assert_eq!(
        points(&reader, "process.memory.virtual"),
        [(none(), 209_715_200.0)]
    );
    assert_eq!(points(&reader, "process.thread.count"), [(none(), 9.0)]);
    assert_eq!(
        points(&reader, "process.unix.file_descriptor.count"),
        [(none(), 12.0)]
    );
    assert_eq!(points(&reader, "tokio.runtime.task.count"), []);
}

#[test]
fn a_process_that_cannot_be_read_reports_no_points() {
    let (reader, provider) = reader_and_provider();
    observe_with(&provider, ABSENT, None);
    for name in [
        "process.cpu.time",
        "process.memory.usage",
        "process.memory.virtual",
        "process.thread.count",
        "process.unix.file_descriptor.count",
    ] {
        assert_eq!(points(&reader, name), [], "{name}");
    }
}

/// Three tasks spawned from outside a current-thread runtime wait, alive,
/// in its global queue; the runtime has one worker.
#[test]
fn the_runtime_reports_its_workers_alive_tasks_and_global_queue() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (release, wait) = tokio::sync::watch::channel(false);
    for _ in 0..3 {
        let mut wait = wait.clone();
        drop(runtime.spawn(async move {
            let _ = wait.wait_for(|go| *go).await;
        }));
    }
    let (reader, provider) = reader_and_provider();
    observe_with(&provider, ABSENT, Some(runtime.handle().clone()));
    let none = String::new;
    assert_eq!(
        points(&reader, "tokio.runtime.worker.count"),
        [(none(), 1.0)]
    );
    assert_eq!(points(&reader, "tokio.runtime.task.count"), [(none(), 3.0)]);
    assert_eq!(
        points(&reader, "tokio.runtime.global_queue.depth"),
        [(none(), 3.0)]
    );
    // Run them to completion: none is alive or queued afterwards.
    release.send_replace(true);
    runtime.block_on(async {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    });
    assert_eq!(points(&reader, "tokio.runtime.task.count"), [(none(), 0.0)]);
    assert_eq!(
        points(&reader, "tokio.runtime.global_queue.depth"),
        [(none(), 0.0)]
    );
}
