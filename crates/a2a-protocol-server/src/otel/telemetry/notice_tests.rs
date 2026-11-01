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
    assert_eq!(
        notices(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318")]),
        Vec::<String>::new()
    );
    assert_eq!(
        notices(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318/otlp")]),
        Vec::<String>::new()
    );
    // The signal's own variable is the complete URL, and ends in the path by design.
    assert_eq!(
        notices(&[(
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            "http://c:4318/v1/traces"
        )]),
        Vec::<String>::new()
    );
    // gRPC has no path to append.
    assert_eq!(
        notices(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4317/v1/traces"),
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
        ]),
        Vec::<String>::new()
    );
    // A signal that does not go over OTLP is not exported to the endpoint.
    assert_eq!(
        notices(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318/v1/traces"),
            ("OTEL_TRACES_EXPORTER", "console"),
        ]),
        Vec::<String>::new()
    );
}

/// The OpenTelemetry specification has an SDK warn and ignore a value it
/// cannot use. A fleet sets one `OTEL_*` preset for every SDK it runs, and a
/// process that refused to start over `zipkin` took all of them down with it
/// (audit C-08, decided 2026-11-01).
#[test]
fn an_exporter_this_crate_lacks_is_noted_and_the_build_goes_on() {
    let t = with_env(&[("OTEL_LOGS_EXPORTER", "zipkin")])
        .build()
        .expect("a name this SDK lacks is not fatal");
    assert!(
        t.notices()
            .iter()
            .any(|n| n.contains("OTEL_LOGS_EXPORTER") && n.contains("`zipkin`")),
        "{:?}",
        t.notices()
    );
    // Nothing usable was left, so the variable counts as unset and logs go
    // where they go by default.
    assert!(t.logger_provider().is_some());
}

/// Every value the specification names and this crate cannot use at once: the
/// build goes on, each variable is named with what was used instead, and what
/// it did use is what the defaults give.
#[test]
fn every_unusable_value_is_noted_with_what_was_used_instead() {
    let t = with_env(&[
        ("OTEL_TRACES_EXPORTER", "jaeger"),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "carrier-pigeon"),
        ("OTEL_EXPORTER_OTLP_COMPRESSION", "zstd"),
        ("OTEL_EXPORTER_OTLP_TIMEOUT", "soon"),
    ])
    .build()
    .expect("none of these is fatal");
    let notices = t.notices().join("\n");
    for (variable, used) in [
        ("OTEL_TRACES_EXPORTER", "the default is used"),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "`http/protobuf` is used"),
        (
            "OTEL_EXPORTER_OTLP_COMPRESSION",
            "the export is not compressed",
        ),
        ("OTEL_EXPORTER_OTLP_TIMEOUT", "10000 is used"),
    ] {
        assert!(
            notices.contains(variable) && notices.contains(used),
            "{variable}/{used} in:\n{notices}"
        );
    }
    // The protocol, compression and timeout are read once per signal, and
    // each variable is the general one: a line each, not three.
    assert_eq!(
        t.notices()
            .iter()
            .filter(|n| n.contains("OTEL_EXPORTER_OTLP_PROTOCOL"))
            .count(),
        1,
        "{notices}"
    );
    assert!(
        t.tracer_provider().is_some(),
        "the default still exports traces"
    );
}

/// `OTEL_SDK_DISABLED=1` is not a disable (the specification reads only
/// `true`), and used to say nothing about it (audit NEW-c-5).
#[test]
fn a_sdk_disabled_that_is_not_true_or_false_is_noted_and_does_not_disable() {
    for value in ["1", "yes", "on"] {
        let t = with_env(&[("OTEL_SDK_DISABLED", value)]).build().unwrap();
        assert!(t.tracer_provider().is_some(), "{value}: not disabled");
        assert!(
            t.notices()
                .iter()
                .any(|n| n.contains("OTEL_SDK_DISABLED") && n.contains("the SDK is not disabled")),
            "{value}: {:?}",
            t.notices()
        );
    }
    for value in ["true", "TRUE", "false", " False "] {
        let t = with_env(&[("OTEL_SDK_DISABLED", value)]).build().unwrap();
        assert!(t.notices().is_empty(), "{value}: {:?}", t.notices());
    }
}

/// The values it still refuses are the ones it cannot do without: a
/// destination it cannot parse, and a file it was told to trust or present.
#[test]
fn what_cannot_be_done_without_still_fails_the_build() {
    // OTLP/HTTP with no scheme: guessing http or https is a guess about TLS.
    let err = with_env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "collector:4318")])
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("needs a scheme"), "{err}");
    // A client certificate without its key would connect without one.
    let err = with_env(&[("OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE", "/c.pem")])
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("needs its key"), "{err}");
}
