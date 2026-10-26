// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a person running a process sees: the `console` exporter on standard
//! output, and on standard error the startup line, warnings, and a lost
//! export. Standard output and error are the process's own, so each test
//! re-runs this binary for one test below, with the environment set on the
//! child (writing the environment is `unsafe`, and this crate forbids it).

use a2a_protocol_server::otel::Telemetry;
use opentelemetry::metrics::MeterProvider as _;

use super::collector::unreachable;

const CHILD: &str = "A2A_CONSOLE_TEST_CHILD";

/// Runs only as the child: starts `Telemetry` from the environment alone,
/// installs its subscriber, and records one span, one metric and some events.
#[test]
fn record_as_the_child_process() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let telemetry = Telemetry::builder()
        .with_default_service_name("telemetry-test")
        .build()
        .expect("build from the environment");
    assert!(telemetry.install_subscriber(), "a fresh process has none");
    {
        let span = tracing::info_span!("unit-of-work", answer = 42);
        let _entered = span.enter();
        tracing::info!("an informational event");
        tracing::warn!("a warning to be seen");
    }
    if let Some(meter) = telemetry.meter_provider() {
        meter
            .meter("telemetry-test")
            .u64_counter("probe.count")
            .build()
            .add(1, &[]);
    }
    // A lost export is reported as it happens; whether `shutdown` also
    // errors is not what these tests are about.
    let _ = telemetry.shutdown();
}

struct Ran {
    stdout: String,
    stderr: String,
}

fn run_child(vars: &[(&str, &str)]) -> Ran {
    let mut child = std::process::Command::new(std::env::current_exe().expect("exe"));
    child
        .args([
            "console::record_as_the_child_process",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env_remove("RUST_LOG");
    for (name, _) in std::env::vars().filter(|(k, _)| k.starts_with("OTEL_")) {
        child.env_remove(name);
    }
    for (k, v) in vars {
        child.env(k, v);
    }
    let out = child.output().expect("child runs");
    let ran = Ran {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    };
    assert!(
        out.status.success() && ran.stdout.contains("1 passed"),
        "child failed: {}{}",
        ran.stdout,
        ran.stderr
    );
    ran
}

/// Spans on standard output and no log exporter, so a warning can only be
/// seen where `install_subscriber` prints it.
const SPANS_ONLY: [(&str, &str); 3] = [
    ("OTEL_TRACES_EXPORTER", "console"),
    ("OTEL_METRICS_EXPORTER", "none"),
    ("OTEL_LOGS_EXPORTER", "none"),
];

const CONSOLE: [(&str, &str); 3] = [
    ("OTEL_TRACES_EXPORTER", "console"),
    ("OTEL_METRICS_EXPORTER", "console"),
    ("OTEL_LOGS_EXPORTER", "console"),
];

#[test]
fn the_console_exporter_prints_each_signal_to_standard_output() {
    let ran = run_child(&CONSOLE);
    let out = &ran.stdout;
    let has = |needle: &str| out.lines().any(|l| l.contains(needle));
    assert!(has(" SPAN unit-of-work internal "), "{out}");
    assert!(has(" answer=42"), "{out}");
    assert!(has(" LOG INFO an informational event"), "{out}");
    assert!(
        !out.contains("busy_ns") && !out.contains("code.file"),
        "{out}"
    );
    assert!(has(" LOG WARN a warning to be seen"), "{out}");
    assert!(has(" METRIC probe.count sum 1 cumulative"), "{out}");
    // Nothing was sent anywhere, so nothing could have been lost.
    assert!(!ran.stderr.contains("export of"), "{}", ran.stderr);
}

#[test]
fn install_subscriber_says_where_telemetry_goes_and_shows_warnings_only() {
    let ran = run_child(&SPANS_ONLY);
    let err = &ran.stderr;
    assert!(
        err.contains("telemetry: traces → console; metrics → off; logs → off"),
        "{err}"
    );
    assert!(
        err.contains("WARN") && err.contains("a warning to be seen"),
        "{err}"
    );
    assert!(!err.contains("an informational event"), "{err}");
}

/// With the console log exporter on, every event is already printed in full
/// on standard output, so it is not printed a second time on standard error.
#[test]
fn a_warning_is_not_printed_twice_when_logs_go_to_the_console() {
    let ran = run_child(&CONSOLE);
    assert!(
        ran.stdout.contains(" LOG WARN a warning to be seen"),
        "{}",
        ran.stdout
    );
    assert!(
        !ran.stderr.contains("a warning to be seen"),
        "{}",
        ran.stderr
    );
    assert!(
        ran.stderr
            .contains("telemetry: traces → console; metrics → console every 60 s; logs → console"),
        "the startup line is still said: {}",
        ran.stderr
    );
}

#[test]
fn rust_log_widens_what_is_printed_to_standard_error() {
    let mut vars = SPANS_ONLY.to_vec();
    vars.push(("RUST_LOG", "info"));
    let ran = run_child(&vars);
    assert!(
        ran.stderr.contains("an informational event"),
        "{}",
        ran.stderr
    );
}

#[test]
fn a_rust_log_that_is_not_understood_is_said_not_ignored() {
    let mut vars = SPANS_ONLY.to_vec();
    vars.push(("RUST_LOG", "a2a=loud"));
    let ran = run_child(&vars);
    assert!(
        ran.stderr.contains("RUST_LOG") && ran.stderr.contains("a2a=loud"),
        "{}",
        ran.stderr
    );
}

/// The case that was silent: a collector that cannot be reached. The process
/// is told, on standard error, which signal and where.
#[test]
fn a_collector_that_cannot_be_reached_is_reported_on_standard_error() {
    let endpoint = unreachable();
    let ran = run_child(&[
        ("OTEL_EXPORTER_OTLP_ENDPOINT", &endpoint),
        ("OTEL_METRICS_EXPORTER", "none"),
        ("OTEL_LOGS_EXPORTER", "none"),
        ("OTEL_BSP_SCHEDULE_DELAY", "100"),
    ]);
    let err = &ran.stderr;
    assert!(
        err.contains(&format!(
            "telemetry: traces → otlp http/protobuf {endpoint}/v1/traces"
        )),
        "{err}"
    );
    assert!(
        err.lines().any(|l| l.contains("WARN")
            && l.contains(&format!(
                "OTLP export of traces to {endpoint}/v1/traces failed"
            ))),
        "{err}"
    );
    // The SDK's own report of the same loss is left out: it repeats every
    // batch, and the line above says it once a while with more to go on.
    assert!(!err.contains("ExportError"), "{err}");
}

#[test]
fn nothing_configured_says_how_to_turn_telemetry_on() {
    let ran = run_child(&[
        ("OTEL_TRACES_EXPORTER", "none"),
        ("OTEL_METRICS_EXPORTER", "none"),
        ("OTEL_LOGS_EXPORTER", "none"),
    ]);
    assert!(
        ran.stderr
            .contains("telemetry: nothing is exported; set OTEL_EXPORTER_OTLP_ENDPOINT"),
        "{}",
        ran.stderr
    );
}
