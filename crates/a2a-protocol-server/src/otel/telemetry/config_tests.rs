// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What `config.rs` reads from the environment, with the environment injected.

use super::*;
use std::collections::HashMap;

fn env(pairs: &[(&str, &str)]) -> Box<Env> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Box::new(move |k| map.get(k).cloned().filter(|v| !v.trim().is_empty()))
}

#[test]
fn sdk_disabled_reads_only_true() {
    assert!(sdk_disabled(&*env(&[("OTEL_SDK_DISABLED", "TRUE")])));
    assert!(!sdk_disabled(&*env(&[("OTEL_SDK_DISABLED", "false")])));
    assert!(!sdk_disabled(&*env(&[("OTEL_SDK_DISABLED", "1")])));
    assert!(!sdk_disabled(&*env(&[])));
}

/// The protocol, and the notes it left.
fn protocol_of(
    e: &Env,
    signal: Signal,
    explicit: Option<OtlpProtocol>,
) -> (OtlpProtocol, Vec<String>) {
    let notes = Notes::default();
    let got = protocol(e, signal, explicit, &notes);
    (got, notes.take())
}

#[test]
fn protocol_prefers_explicit_then_signal_then_general_then_http() {
    let e = env(&[
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
        ("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "http/protobuf"),
    ]);
    assert_eq!(
        protocol_of(&*e, Signal::Traces, None),
        (OtlpProtocol::HttpProtobuf, vec![])
    );
    assert_eq!(
        protocol_of(&*e, Signal::Metrics, None),
        (OtlpProtocol::Grpc, vec![])
    );
    assert_eq!(
        protocol_of(&*e, Signal::Metrics, Some(OtlpProtocol::HttpProtobuf)),
        (OtlpProtocol::HttpProtobuf, vec![])
    );
    assert_eq!(
        protocol_of(&*env(&[]), Signal::Logs, None),
        (OtlpProtocol::HttpProtobuf, vec![])
    );
}

/// The names are matched without regard to case (audit C-08).
#[test]
fn protocol_names_are_read_in_any_case() {
    for (value, want) in [
        ("GRPC", OtlpProtocol::Grpc),
        (" Grpc ", OtlpProtocol::Grpc),
        ("HTTP/Protobuf", OtlpProtocol::HttpProtobuf),
    ] {
        let e = env(&[("OTEL_EXPORTER_OTLP_PROTOCOL", value)]);
        assert_eq!(
            protocol_of(&*e, Signal::Traces, None),
            (want, vec![]),
            "{value:?}"
        );
    }
}

#[cfg(not(feature = "otlp-json"))]
#[test]
fn protocol_json_without_the_feature_is_noted_naming_both_and_protobuf_is_used() {
    let e = env(&[("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json")]);
    let (got, notes) = protocol_of(&*e, Signal::Traces, None);
    assert_eq!(got, OtlpProtocol::HttpProtobuf);
    let msg = &notes[0];
    assert!(
        msg.contains("OTEL_EXPORTER_OTLP_PROTOCOL")
            && msg.contains("http/json")
            && msg.contains("otlp-json")
            && msg.contains("`http/protobuf` is used"),
        "{msg}"
    );
}

#[cfg(feature = "otlp-json")]
#[test]
fn protocol_reads_json_with_the_feature() {
    let e = env(&[("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL", "http/json")]);
    assert_eq!(
        protocol_of(&*e, Signal::Logs, None),
        (OtlpProtocol::HttpJson, vec![])
    );
    assert!(OtlpProtocol::HttpJson.is_http());
}

#[test]
fn protocol_garbage_is_noted_naming_the_variable_and_protobuf_is_used() {
    assert!(!OtlpProtocol::Grpc.is_http() && OtlpProtocol::HttpProtobuf.is_http());
    let e = env(&[("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL", "udp")]);
    let (got, notes) = protocol_of(&*e, Signal::Logs, None);
    assert_eq!(got, OtlpProtocol::HttpProtobuf);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL")
            && notes[0].contains("udp")
            && notes[0].contains("`http/protobuf` is used"),
        "{notes:?}"
    );
}

/// The general variable is read by three signals; its line is kept once.
#[test]
fn one_bad_variable_read_by_three_signals_is_noted_once() {
    let e = env(&[("OTEL_EXPORTER_OTLP_PROTOCOL", "udp")]);
    let notes = Notes::default();
    for signal in [Signal::Traces, Signal::Metrics, Signal::Logs] {
        assert_eq!(
            protocol(&*e, signal, None, &notes),
            OtlpProtocol::HttpProtobuf
        );
    }
    assert_eq!(notes.take().len(), 1);
    assert!(notes.take().is_empty(), "taking leaves none");
}

/// An explicit protocol from code is not second-guessed by a bad variable.
#[test]
fn code_wins_over_a_bad_variable_without_a_notice() {
    let e = env(&[("OTEL_EXPORTER_OTLP_PROTOCOL", "udp")]);
    assert_eq!(
        protocol_of(&*e, Signal::Traces, Some(OtlpProtocol::Grpc)),
        (OtlpProtocol::Grpc, vec![])
    );
}

#[test]
fn timeout_is_milliseconds_with_a_ten_second_default() {
    let notes = Notes::default();
    assert_eq!(
        timeout(&*env(&[]), Signal::Traces, &notes),
        Duration::from_secs(10)
    );
    let e = env(&[
        ("OTEL_EXPORTER_OTLP_TIMEOUT", "2500"),
        ("OTEL_EXPORTER_OTLP_LOGS_TIMEOUT", "100"),
    ]);
    assert_eq!(
        timeout(&*e, Signal::Traces, &notes),
        Duration::from_millis(2500)
    );
    assert_eq!(
        timeout(&*e, Signal::Logs, &notes),
        Duration::from_millis(100)
    );
    assert!(notes.take().is_empty(), "good values leave no notice");
}

#[test]
fn a_timeout_that_is_not_milliseconds_is_noted_and_ten_seconds_used() {
    let notes = Notes::default();
    let e = env(&[("OTEL_EXPORTER_OTLP_TIMEOUT", "5s")]);
    assert_eq!(
        timeout(&*e, Signal::Traces, &notes),
        Duration::from_secs(10)
    );
    let lines = notes.take();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("OTEL_EXPORTER_OTLP_TIMEOUT")
            && lines[0].contains("5s")
            && lines[0].contains("10000 is used"),
        "{lines:?}"
    );
}

#[test]
fn service_name_counts_from_either_variable_and_not_from_others() {
    assert!(service_name_from_env(&*env(&[("OTEL_SERVICE_NAME", "a")])));
    assert!(service_name_from_env(&*env(&[(
        "OTEL_RESOURCE_ATTRIBUTES",
        "deployment.environment.name=prod, service.name=b"
    )])));
    assert!(!service_name_from_env(&*env(&[(
        "OTEL_RESOURCE_ATTRIBUTES",
        "service.namespace=x"
    )])));
    assert!(!service_name_from_env(&*env(&[])));
}

#[test]
fn a_client_identity_takes_both_files_each_falling_back_on_its_own() {
    assert!(
        client_identity_files(&*env(&[]), Signal::Traces)
            .unwrap()
            .is_none()
    );
    let e = env(&[
        ("OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE", "/c.pem"),
        ("OTEL_EXPORTER_OTLP_CLIENT_KEY", "/k.pem"),
        ("OTEL_EXPORTER_OTLP_TRACES_CLIENT_KEY", "/tk.pem"),
    ]);
    let [cert, key] = client_identity_files(&*e, Signal::Traces).unwrap().unwrap();
    assert_eq!(cert.1, "/c.pem");
    assert_eq!(
        key,
        (
            "OTEL_EXPORTER_OTLP_TRACES_CLIENT_KEY".to_owned(),
            "/tk.pem".to_owned()
        )
    );
    let [_, key] = client_identity_files(&*e, Signal::Logs).unwrap().unwrap();
    assert_eq!(key.1, "/k.pem");
}

#[test]
fn half_a_client_identity_is_refused_naming_the_variable_set() {
    let e = env(&[("OTEL_EXPORTER_OTLP_METRICS_CLIENT_CERTIFICATE", "/c.pem")]);
    let msg = client_identity_files(&*e, Signal::Metrics)
        .unwrap_err()
        .to_string();
    assert!(
        msg.contains("OTEL_EXPORTER_OTLP_METRICS_CLIENT_CERTIFICATE") && msg.contains("KEY"),
        "{msg}"
    );
    let e = env(&[("OTEL_EXPORTER_OTLP_CLIENT_KEY", "/k.pem")]);
    let msg = client_identity_files(&*e, Signal::Logs)
        .unwrap_err()
        .to_string();
    assert!(
        msg.contains("OTEL_EXPORTER_OTLP_CLIENT_KEY") && msg.contains("CERTIFICATE"),
        "{msg}"
    );
}

#[test]
fn an_empty_value_is_unset() {
    let e = env(&[("OTEL_SERVICE_NAME", "")]);
    assert!(!service_name_from_env(&*e));
}
