// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Export through an HTTP proxy, on every transport.
//!
//! Each collector is addressed by a host no resolver knows
//! (`collector.invalid`) or by `localhost`, and the proxy maps every host to
//! loopback, so an export that arrives went through the proxy — and the
//! proxy's own log says how: an absolute-form `POST` for plain OTLP/HTTP, a
//! `CONNECT` for TLS and for gRPC.

use a2a_protocol_server::otel::{OtlpProtocol, Telemetry};
use base64::Engine as _;

use super::certs::test_certs;
use super::collector::Collector;
use super::forward_proxy::ForwardProxy;
use super::{assert_all_three_arrived, builder, record_one_of_each};

fn unresolvable(collector: &Collector) -> String {
    format!("http://collector.invalid:{}", collector.addr.port())
}

#[test]
fn plain_http_is_forwarded_as_absolute_form_requests() {
    let collector = Collector::http();
    let proxy = ForwardProxy::start();
    let base = unresolvable(&collector);
    let telemetry = builder(&base, OtlpProtocol::HttpProtobuf)
        .with_proxy(proxy.url())
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
    let mut posted = proxy.targets("POST");
    posted.sort();
    posted.dedup();
    assert_eq!(
        posted,
        [
            format!("{base}/v1/logs"),
            format!("{base}/v1/metrics"),
            format!("{base}/v1/traces"),
        ]
    );
}

#[test]
fn a_login_in_the_proxy_url_is_sent_as_proxy_authorization() {
    let collector = Collector::http();
    let proxy = ForwardProxy::start();
    let telemetry = builder(&unresolvable(&collector), OtlpProtocol::HttpProtobuf)
        .with_proxy(proxy.url_with_login("user", "pa%20ss"))
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("user:pa ss")
    );
    let forwarded: Vec<_> = proxy
        .seen()
        .into_iter()
        .filter(|p| p.method == "POST")
        .collect();
    assert!(!forwarded.is_empty());
    for p in forwarded {
        assert_eq!(
            p.headers.get("proxy-authorization").map(String::as_str),
            Some(expected.as_str()),
            "{}",
            p.target
        );
    }
}

#[test]
fn https_goes_through_a_connect_tunnel_with_the_certificate_still_checked() {
    let certs = test_certs();
    let collector = Collector::https(&certs);
    let proxy = ForwardProxy::start();
    let telemetry = builder(&collector.https_base(), OtlpProtocol::HttpProtobuf)
        .with_ca_certificate_pem(&certs.ca_pem)
        .with_proxy(proxy.url())
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
    let tunnels = proxy.targets("CONNECT");
    assert!(!tunnels.is_empty(), "{:?}", proxy.seen());
    for t in tunnels {
        assert_eq!(t, format!("localhost:{}", collector.addr.port()));
    }
}

/// The control for the test above: a tunnel carries the TLS handshake end to
/// end, so a client that does not trust the collector's CA is refused inside
/// it — the proxy being there does not switch verification off.
#[test]
fn a_tunnel_does_not_skip_certificate_verification() {
    let certs = test_certs();
    let collector = Collector::https(&certs);
    let proxy = ForwardProxy::start();
    let telemetry = builder(&collector.https_base(), OtlpProtocol::HttpProtobuf)
        .with_proxy(proxy.url())
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    let _ = telemetry.shutdown();
    assert!(
        !proxy.targets("CONNECT").is_empty(),
        "never reached the proxy"
    );
    assert!(collector.received().is_empty(), "{:?}", collector.paths());
}

#[test]
fn plaintext_grpc_is_tunnelled() {
    let collector = Collector::grpc();
    let proxy = ForwardProxy::start();
    let base = unresolvable(&collector);
    let telemetry = builder(&base, OtlpProtocol::Grpc)
        .with_proxy(proxy.url())
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
    let tunnels = proxy.targets("CONNECT");
    assert!(!tunnels.is_empty(), "{:?}", proxy.seen());
    for t in tunnels {
        assert_eq!(t, format!("collector.invalid:{}", collector.addr.port()));
    }
}

#[test]
fn grpc_over_tls_is_tunnelled_with_the_certificate_still_checked() {
    let certs = test_certs();
    let collector = Collector::grpcs(&certs);
    let proxy = ForwardProxy::start();
    let telemetry = builder(&collector.https_base(), OtlpProtocol::Grpc)
        .with_ca_certificate_pem(&certs.ca_pem)
        .with_proxy(proxy.url())
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
    assert!(!proxy.targets("CONNECT").is_empty(), "{:?}", proxy.seen());

    let untrusting = Collector::grpcs(&certs);
    let telemetry = builder(&untrusting.https_base(), OtlpProtocol::Grpc)
        .with_proxy(proxy.url())
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    let _ = telemetry.shutdown();
    assert!(untrusting.received().is_empty());
}

/// `without_proxy` after `with_proxy` leaves the last word to the last call,
/// and a collector that is reachable directly is then reached directly.
#[test]
fn without_proxy_sends_straight() {
    let collector = Collector::http();
    let proxy = ForwardProxy::start();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .with_proxy(proxy.url())
        .without_proxy()
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
    assert!(proxy.seen().is_empty(), "{:?}", proxy.seen());
}

/// A proxy of a kind this crate does not speak is ignored, with a warning,
/// and the export goes direct instead of the process refusing to start.
#[test]
fn an_unsupported_proxy_kind_exports_direct() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .with_proxy("socks5://127.0.0.1:1")
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
}

// ── The process environment, through a child ─────────────────────────────────
//
// Writing the environment is `unsafe` in edition 2024 and this crate forbids
// `unsafe`, so the variables are set on a child process: this test binary,
// re-run for the one test below, which reads nothing but the environment.

const CHILD: &str = "A2A_PROXY_TEST_CHILD";

/// Runs only as the child: exports one of each signal to wherever
/// `OTEL_EXPORTER_OTLP_ENDPOINT` says, through whatever proxy the
/// environment names.
#[test]
fn export_as_the_child_process() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let telemetry = Telemetry::builder()
        .install_globally(false)
        .with_default_service_name("telemetry-test")
        .build()
        .expect("build from the environment");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
}

/// Re-runs `export_as_the_child_process` with `vars` set and every proxy and
/// OpenTelemetry variable of this machine cleared.
fn run_child(vars: &[(&str, &str)]) {
    let mut child = std::process::Command::new(std::env::current_exe().expect("exe"));
    child
        .args([
            "proxy::export_as_the_child_process",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, "1");
    for name in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "REQUEST_METHOD",
    ] {
        child.env_remove(name);
    }
    for (name, _) in std::env::vars().filter(|(k, _)| k.starts_with("OTEL_")) {
        child.env_remove(name);
    }
    for (k, v) in vars {
        child.env(k, v);
    }
    let out = child.output().expect("child runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "child failed: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn http_proxy_in_the_environment_carries_the_export() {
    let collector = Collector::http();
    let proxy = ForwardProxy::start();
    let endpoint = unresolvable(&collector);
    run_child(&[
        ("OTEL_EXPORTER_OTLP_ENDPOINT", &endpoint),
        ("HTTP_PROXY", &proxy.url()),
    ]);
    assert_all_three_arrived(&collector);
    assert!(
        proxy
            .targets("POST")
            .contains(&format!("{endpoint}/v1/traces")),
        "{:?}",
        proxy.seen()
    );
}

#[test]
fn no_proxy_in_the_environment_keeps_the_export_direct() {
    let proxy = ForwardProxy::start();

    // Without `NO_PROXY`, loopback goes through the proxy …
    let through = Collector::http();
    run_child(&[
        ("OTEL_EXPORTER_OTLP_ENDPOINT", &through.http_base()),
        ("HTTP_PROXY", &proxy.url()),
    ]);
    assert_all_three_arrived(&through);
    let carried = proxy.seen().len();
    assert!(carried > 0);

    // … and with it, straight, leaving the proxy's log as it was.
    let direct = Collector::http();
    run_child(&[
        ("OTEL_EXPORTER_OTLP_ENDPOINT", &direct.http_base()),
        ("HTTP_PROXY", &proxy.url()),
        ("NO_PROXY", "127.0.0.1"),
    ]);
    assert_all_three_arrived(&direct);
    assert_eq!(proxy.seen().len(), carried);
}
