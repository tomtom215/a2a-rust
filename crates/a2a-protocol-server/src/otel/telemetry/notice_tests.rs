// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The warnings `Telemetry::notices` carries for a configuration that
//! builds and does not do what its author meant, with the environment
//! injected.

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

/// The notices of a build with traces over OTLP and the other signals off.
fn notices(pairs: &[(&str, &str)]) -> Vec<String> {
    with_env(pairs)
        .with_metrics(false)
        .with_logs(false)
        .build()
        .expect("builds")
        .notices()
        .to_vec()
}

#[test]
fn a_base_endpoint_that_already_ends_in_a_signal_path_is_warned_about() {
    let got = notices(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318/v1/traces")]);
    assert_eq!(got.len(), 1, "{got:?}");
    let said = &got[0];
    assert!(
        said.starts_with("OTEL_EXPORTER_OTLP_ENDPOINT is http://c:4318/v1/traces"),
        "{said}"
    );
    assert!(
        said.contains("would go to http://c:4318/v1/traces/v1/traces"),
        "{said}"
    );
    assert!(said.contains("Give the base URL (http://c:4318)"), "{said}");
    assert!(
        said.contains("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"),
        "{said}"
    );
}

#[test]
fn a_trailing_slash_and_the_other_signals_paths_are_caught_too() {
    for endpoint in [
        "http://c:4318/v1/traces/",
        "http://c:4318/v1/metrics",
        "http://c/otlp/v1/logs",
    ] {
        let got = notices(&[("OTEL_EXPORTER_OTLP_ENDPOINT", endpoint)]);
        assert_eq!(got.len(), 1, "{endpoint}: {got:?}");
    }
}

#[test]
fn an_endpoint_given_in_code_is_checked_like_the_variable() {
    let got = with_env(&[])
        .with_otlp_endpoint("https://c.example/v1/traces")
        .with_metrics(false)
        .with_logs(false)
        .build()
        .expect("builds")
        .notices()
        .to_vec();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].starts_with("the endpoint given in code is"),
        "{}",
        got[0]
    );
}

#[test]
fn a_base_url_a_signals_own_variable_or_grpc_is_left_alone() {
    // A base with no signal path: what the specification expects.
    assert!(notices(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318")]).is_empty());
    assert!(notices(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318/otlp")]).is_empty());
    // The signal's own variable is the complete URL, and ends in the path by design.
    assert!(
        notices(&[(
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            "http://c:4318/v1/traces"
        )])
        .is_empty()
    );
    // gRPC has no path to append.
    assert!(
        notices(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4317/v1/traces"),
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
        ])
        .is_empty()
    );
    // A signal that does not go over OTLP is not exported to the endpoint.
    assert!(
        notices(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318/v1/traces"),
            ("OTEL_TRACES_EXPORTER", "console"),
        ])
        .is_empty()
    );
}
