// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The builder's decisions, with the environment injected. Export itself is
//! tested end to end against stand-in collectors in `tests/telemetry_*.rs`.

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
fn self_targets_match_whole_path_segments_only() {
    assert!(is_self_target("hyper"));
    assert!(is_self_target("hyper::proto::h1"));
    assert!(is_self_target("opentelemetry_sdk::trace"));
    assert!(!is_self_target("hyperlane"));
    assert!(!is_self_target("my_agent::hyper"));
    assert!(!is_self_target("a2a_protocol_server::rpc"));
}

#[test]
fn sdk_disabled_builds_no_provider_and_no_thread() {
    let t = with_env(&[
        ("OTEL_SDK_DISABLED", "true"),
        ("OTEL_TRACES_EXPORTER", "otlp"),
    ])
    .build()
    .unwrap();
    assert!(t.tracer_provider().is_none());
    assert!(t.meter_provider().is_none());
    assert!(t.logger_provider().is_none());
    assert!(t.runtime.is_none());
}

#[test]
fn every_signal_is_on_by_default_and_each_variable_turns_one_off() {
    let t = with_env(&[]).build().unwrap();
    assert!(
        t.tracer_provider().is_some()
            && t.meter_provider().is_some()
            && t.logger_provider().is_some()
    );
    let t = with_env(&[("OTEL_METRICS_EXPORTER", "none")])
        .build()
        .unwrap();
    assert!(
        t.tracer_provider().is_some()
            && t.meter_provider().is_none()
            && t.logger_provider().is_some()
    );
}

#[test]
fn langfuse_defaults_metrics_and_logs_off_and_the_environment_can_restore_them() {
    let lf = Langfuse::new("http://127.0.0.1:9", "pk", "sk");
    let t = with_env(&[]).with_langfuse(lf.clone()).build().unwrap();
    assert!(t.tracer_provider().is_some());
    assert!(t.meter_provider().is_none() && t.logger_provider().is_none());
    let t = with_env(&[("OTEL_LOGS_EXPORTER", "otlp")])
        .with_langfuse(lf)
        .build()
        .unwrap();
    assert!(t.logger_provider().is_some() && t.meter_provider().is_none());
}

#[test]
fn langfuse_traces_ignore_a_grpc_protocol_variable() {
    let b = with_env(&[("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")]).with_langfuse(Langfuse::new(
        "https://cloud.langfuse.com/",
        "pk",
        "sk",
    ));
    let traces = b.target(Signal::Traces, Arc::default()).unwrap();
    assert_eq!(traces.protocol, OtlpProtocol::HttpProtobuf);
    assert_eq!(
        traces.endpoint.as_deref(),
        Some("https://cloud.langfuse.com/api/public/otel/v1/traces")
    );
    // The other signals still follow the environment.
    assert_eq!(
        b.target(Signal::Metrics, Arc::default()).unwrap().protocol,
        OtlpProtocol::Grpc
    );
}

#[test]
fn a_bad_variable_fails_the_build_naming_it() {
    let err = with_env(&[("OTEL_LOGS_EXPORTER", "console")])
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("OTEL_LOGS_EXPORTER"), "{err}");
}

#[test]
fn an_unreadable_certificate_file_fails_the_build_naming_the_variable() {
    let err = with_env(&[("OTEL_EXPORTER_OTLP_CERTIFICATE", "/nonexistent/ca.pem")])
        .build()
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("OTEL_EXPORTER_OTLP_CERTIFICATE") && msg.contains("/nonexistent/ca.pem"),
        "{msg}"
    );
}

#[test]
fn a_certificate_that_is_not_pem_is_refused() {
    let err = with_env(&[])
        .with_ca_certificate_pem("not a certificate")
        .build()
        .unwrap_err();
    assert!(matches!(err, TelemetryError::Tls(_)), "{err}");
}

#[test]
fn the_default_service_name_applies_only_when_the_environment_names_none() {
    let named = with_env(&[])
        .with_default_service_name("code-default")
        .resource();
    assert_eq!(
        named
            .get(&opentelemetry::Key::new("service.name"))
            .map(|v| v.to_string()),
        Some("code-default".to_owned())
    );
    // The injected environment says the service is named, so the default
    // must not be applied; the process environment, which `Resource` reads,
    // names none here, so the SDK's own fallback shows through.
    let deferred = with_env(&[("OTEL_SERVICE_NAME", "from-env")])
        .with_default_service_name("code-default")
        .resource();
    let name = deferred
        .get(&opentelemetry::Key::new("service.name"))
        .map(|v| v.to_string())
        .unwrap_or_default();
    assert_ne!(name, "code-default");
}

#[test]
fn debug_output_never_contains_header_values() {
    let b = with_env(&[]).with_otlp_header("authorization", "Basic c2VjcmV0");
    assert!(!format!("{b:?}").contains("c2VjcmV0"));
}
