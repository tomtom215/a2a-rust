// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The helpers on `Target` that need no runtime: what is shown of a URL, which
//! endpoint an exporter will use, and whether it is dialled through a proxy.
//! Export itself is tested against stand-in collectors in
//! `tests/telemetry_export/`.

use super::*;

#[test]
fn a_login_in_a_url_is_not_displayed() {
    assert_eq!(
        without_userinfo("https://user:p%40ss@collector.example:4318/v1/traces"),
        "https://collector.example:4318/v1/traces"
    );
    assert_eq!(without_userinfo("http://c:4317"), "http://c:4317");
    assert_eq!(
        without_userinfo("https://tok@c.example"),
        "https://c.example"
    );
    assert_eq!(without_userinfo("not a url"), "not a url");
}

#[test]
fn the_destination_is_the_endpoint_the_exporter_will_use() {
    let none = |_: &str| None;
    let general =
        |k: &str| (k == "OTEL_EXPORTER_OTLP_ENDPOINT").then(|| "http://g:4318/".to_owned());
    let own =
        |k: &str| (k == "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT").then(|| "http://own:1/x".to_owned());
    let d = |env: &Env, signal, protocol, endpoint| {
        Target::destination_for(env, signal, protocol, endpoint)
    };
    assert_eq!(
        d(&none, Signal::Traces, OtlpProtocol::HttpProtobuf, None),
        "http://localhost:4318/v1/traces"
    );
    assert_eq!(
        d(&none, Signal::Traces, OtlpProtocol::Grpc, None),
        "http://localhost:4317"
    );
    assert_eq!(
        d(&general, Signal::Metrics, OtlpProtocol::HttpProtobuf, None),
        "http://g:4318/v1/metrics"
    );
    assert_eq!(
        d(&own, Signal::Logs, OtlpProtocol::HttpProtobuf, None),
        "http://own:1/x"
    );
    assert_eq!(
        d(
            &general,
            Signal::Logs,
            OtlpProtocol::HttpProtobuf,
            Some("http://code/v1/logs")
        ),
        "http://code/v1/logs"
    );
}

/// A target whose proxy rules come from `pairs`, as the environment.
fn target_with(pairs: &[(&str, &str)]) -> Target {
    let map: std::collections::HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    let env = move |k: &str| map.get(k).cloned();
    Target {
        signal: Signal::Traces,
        protocol: OtlpProtocol::Grpc,
        endpoint: None,
        headers: Vec::new(),
        compression: None,
        proxy: Arc::new(proxy::matcher(&env, &proxy::ProxySetting::Environment)),
        destination: String::new(),
        cause: Arc::default(),
        timeout: Duration::from_secs(1),
        ca_pem: Vec::new(),
        client_identity: None,
    }
}

/// Only an endpoint a proxy rule applies to is dialled through the proxy.
#[test]
fn an_endpoint_is_proxied_only_when_a_rule_applies_to_it() {
    let via = target_with(&[("ALL_PROXY", "http://p:3128")]);
    assert!(via.proxied("http://c:4317"));
    assert!(via.proxied("https://c:4317"));
    assert!(!target_with(&[]).proxied("http://c:4317"));
    let exempt = target_with(&[("ALL_PROXY", "http://p:3128"), ("NO_PROXY", "c")]);
    assert!(!exempt.proxied("http://c:4317"));
    assert!(!via.proxied("not a uri"));
}

#[test]
fn a_base_endpoint_gains_the_signal_path_on_http_only() {
    assert_eq!(
        Target::endpoint_from_base("http://c:4318/", Signal::Logs, OtlpProtocol::HttpProtobuf),
        "http://c:4318/v1/logs"
    );
    assert_eq!(
        Target::endpoint_from_base("http://c:4317", Signal::Logs, OtlpProtocol::Grpc),
        "http://c:4317"
    );
}

/// An IPv6 literal endpoint keeps its brackets and port through every step
/// that does not dial: the signal path, the reported route and the gRPC
/// channel. Dialling one was not tested: the host these were measured on
/// has no IPv6 in its kernel.
#[tokio::test]
async fn an_ipv6_endpoint_is_built_as_written() {
    assert_eq!(
        Target::endpoint_from_base(
            "http://[::1]:4318/",
            Signal::Traces,
            OtlpProtocol::HttpProtobuf
        ),
        "http://[::1]:4318/v1/traces"
    );
    let general =
        |k: &str| (k == "OTEL_EXPORTER_OTLP_ENDPOINT").then(|| "https://[fd00::5]:4318".to_owned());
    assert_eq!(
        Target::destination_for(&general, Signal::Logs, OtlpProtocol::HttpProtobuf, None),
        "https://[fd00::5]:4318/v1/logs"
    );
    let route = super::super::hints::Route::of(OtlpProtocol::Grpc, "https://[::1]:4417");
    assert!(route.https);
    assert_eq!(route.port, Some(4417));
    let target = target_with(&[]);
    for endpoint in ["http://[::1]:4317", "https://[::1]:4417"] {
        assert!(
            target.grpc_channel(endpoint).is_ok(),
            "{endpoint}: the channel is built (it connects on first use)"
        );
    }
}
