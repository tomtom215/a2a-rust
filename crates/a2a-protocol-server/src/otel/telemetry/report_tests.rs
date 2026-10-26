// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What `Telemetry::describe` says, and which signals get which exporters,
//! with the environment injected.

use std::collections::HashMap;

use super::*;

fn with_env(pairs: &[(&str, &str)]) -> TelemetryBuilder {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Telemetry::builder()
        .install_globally(false)
        .with_env(move |k| map.get(k).cloned())
}

#[test]
fn a_console_only_process_starts_no_export_thread() {
    let t = with_env(&[
        ("OTEL_TRACES_EXPORTER", "console"),
        ("OTEL_METRICS_EXPORTER", "none"),
        ("OTEL_LOGS_EXPORTER", "none"),
    ])
    .build()
    .unwrap();
    assert!(t.tracer_provider().is_some());
    assert!(t.meter_provider().is_none() && t.logger_provider().is_none());
    assert!(t.runtime.is_none(), "nothing is sent, so no thread");
    assert_eq!(
        t.describe(),
        "telemetry: traces → console; metrics → off; logs → off"
    );
}

#[test]
fn a_signal_can_go_to_a_collector_and_the_console_at_once() {
    let t = with_env(&[
        ("OTEL_TRACES_EXPORTER", "otlp,console"),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
        ("OTEL_METRIC_EXPORT_INTERVAL", "5000"),
        ("OTEL_METRICS_EXPORTER", "console"),
        ("OTEL_LOGS_EXPORTER", "none"),
    ])
    .build()
    .unwrap();
    assert!(t.runtime.is_some());
    assert_eq!(
        t.describe(),
        "telemetry: traces → otlp http/protobuf http://collector:4318/v1/traces + console; \
         metrics → console every 5 s; logs → off"
    );
}

#[test]
fn describe_names_gzip_and_the_proxy_and_never_a_login() {
    let t = with_env(&[("OTEL_EXPORTER_OTLP_COMPRESSION", "gzip")])
        .with_otlp_endpoint("https://user:secret@collector.example:4318")
        .with_proxy("http://pu:ps@proxy.example:3128")
        .with_metrics(false)
        .with_logs(false)
        .build()
        .unwrap();
    let said = t.describe();
    assert!(
        said.contains(
            "traces → otlp http/protobuf https://collector.example:4318/v1/traces, gzip, \
             via proxy http://proxy.example:3128"
        ),
        "{said}"
    );
    assert!(!said.contains("secret") && !said.contains("ps@"), "{said}");
}

#[test]
fn describe_says_how_to_turn_telemetry_on_when_nothing_is_exported() {
    let t = with_env(&[])
        .with_export_where_configured()
        .build()
        .unwrap();
    let said = t.describe();
    assert!(
        said.starts_with("telemetry: nothing is exported")
            && said.contains("OTEL_EXPORTER_OTLP_ENDPOINT")
            && said.contains("OTEL_TRACES_EXPORTER=console"),
        "{said}"
    );
}

#[test]
fn describe_says_when_the_sdk_is_disabled() {
    let t = with_env(&[("OTEL_SDK_DISABLED", "true")]).build().unwrap();
    assert_eq!(t.describe(), "telemetry: disabled by OTEL_SDK_DISABLED");
}

#[test]
fn an_exporter_named_in_the_environment_overrides_a_signal_turned_off_in_code() {
    let t = with_env(&[("OTEL_TRACES_EXPORTER", "console")])
        .with_traces(false)
        .with_metrics(false)
        .with_logs(false)
        .build()
        .unwrap();
    assert!(t.tracer_provider().is_some(), "{}", t.describe());
}

/// Under `OTEL_SDK_DISABLED` nothing is built, so nothing is installed and
/// the answer says so. (The case where one is installed is
/// `tests/telemetry_install_subscriber.rs`, which owns the global slot.)
#[test]
fn install_subscriber_installs_nothing_when_the_sdk_is_disabled() {
    let t = with_env(&[("OTEL_SDK_DISABLED", "true")]).build().unwrap();
    assert!(!t.install_subscriber());
}

#[test]
fn an_unsupported_propagator_is_a_notice_not_an_error() {
    let t = with_env(&[("OTEL_PROPAGATORS", "tracecontext,b3")])
        .build()
        .unwrap();
    assert_eq!(t.notices().len(), 1, "{:?}", t.notices());
    assert!(t.notices()[0].contains("b3"), "{:?}", t.notices());
    assert!(with_env(&[]).build().unwrap().notices().is_empty());
}

/// The resource says where the process runs, unless told not to, and
/// `OTEL_RESOURCE_ATTRIBUTES` has the last word on every key.
#[test]
fn the_resource_describes_the_process_and_the_environment_wins() {
    use opentelemetry::Key;
    let has = |r: &Resource, k: &str| r.get(&Key::from(k.to_owned())).map(|v| v.to_string());
    let detected = with_env(&[("HOSTNAME", "pod-1")]).resource();
    assert_eq!(has(&detected, "host.name").as_deref(), Some("pod-1"));
    assert_eq!(
        has(&detected, "process.pid"),
        Some(std::process::id().to_string())
    );
    assert!(has(&detected, "service.instance.id").is_some());

    // A key `OTEL_RESOURCE_ATTRIBUTES` names is not added here, so the SDK's
    // own reading of the variable (from the process environment, which a
    // unit test cannot set) is the only value there is. Checked end to end
    // against `hello-agent` in the commit that added this.
    let named = with_env(&[
        ("HOSTNAME", "pod-1"),
        (
            "OTEL_RESOURCE_ATTRIBUTES",
            "host.name=from-env,process.pid=7",
        ),
    ])
    .resource();
    assert_eq!(has(&named, "host.name"), None, "detected over the variable");
    assert_eq!(
        has(&named, "process.pid"),
        None,
        "detected over the variable"
    );
    assert!(
        has(&named, "os.type").is_some(),
        "keys it does not name stay"
    );

    let plain = with_env(&[("HOSTNAME", "pod-1")])
        .without_resource_detection()
        .resource();
    for key in ["host.name", "host.arch", "os.type", "process.pid"] {
        assert_eq!(has(&plain, key), None, "{key}");
    }
    assert!(has(&plain, "service.instance.id").is_some());
}

/// OTLP/HTTP has no `INSECURE` variable to say which scheme was meant, so a
/// bare `host:port` is an error that says what to write — where
/// `opentelemetry-otlp` said "invalid format". gRPC reads it as it always did.
#[test]
fn a_schemeless_http_endpoint_is_refused_with_what_to_write() {
    for (env, code) in [
        (
            vec![("OTEL_EXPORTER_OTLP_ENDPOINT", "127.0.0.1:4318")],
            None,
        ),
        (
            vec![("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", "collector:4318")],
            None,
        ),
        (vec![], Some("127.0.0.1:4318")),
    ] {
        let mut b = with_env(&env);
        if let Some(base) = code {
            b = b.with_otlp_endpoint(base);
        }
        let msg = b.build().unwrap_err().to_string();
        assert!(
            msg.contains("needs a scheme") && msg.contains("http://") && msg.contains("https://"),
            "{msg}"
        );
    }
    let named = with_env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "127.0.0.1:4318")])
        .build()
        .unwrap_err()
        .to_string();
    assert!(
        named.contains("OTEL_EXPORTER_OTLP_ENDPOINT") && named.contains("http://127.0.0.1:4318"),
        "{named}"
    );

    let grpc = with_env(&[
        ("OTEL_EXPORTER_OTLP_ENDPOINT", "127.0.0.1:4317"),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
        ("OTEL_EXPORTER_OTLP_INSECURE", "true"),
    ])
    .build();
    assert!(grpc.is_ok(), "{:?}", grpc.err());
}
