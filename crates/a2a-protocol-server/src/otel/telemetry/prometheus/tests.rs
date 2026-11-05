// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The scrape endpoint, over real sockets.

use std::collections::HashMap;
use std::io::{Read as _, Write as _};

use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider as _;

use super::*;
use crate::otel::telemetry::Telemetry;

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + Send + Sync + 'static {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |k| map.get(k).cloned()
}

/// One request and the whole response, over a plain socket.
fn scrape(addr: SocketAddr, request: &str) -> String {
    let mut stream = std::net::TcpStream::connect(addr).expect("connect");
    stream
        .write_all(format!("{request}\r\nHost: t\r\nConnection: close\r\n\r\n").as_bytes())
        .expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    response
}

const LISTEN: [(&str, &str); 2] = [
    ("OTEL_EXPORTER_PROMETHEUS_HOST", "127.0.0.1"),
    ("OTEL_EXPORTER_PROMETHEUS_PORT", "0"),
];

fn prometheus_only(extra: &[(&str, &str)]) -> Telemetry {
    let mut pairs = vec![
        ("OTEL_TRACES_EXPORTER", "none"),
        ("OTEL_LOGS_EXPORTER", "none"),
        ("OTEL_METRICS_EXPORTER", "prometheus"),
    ];
    pairs.extend_from_slice(extra);
    Telemetry::builder()
        .install_globally(false)
        .with_default_service_name("scrape-test")
        .with_env(env(&pairs))
        .build()
        .expect("builds")
}

/// The metrics a process recorded are what a scrape returns, in the
/// text format, under the names the book's Prometheus table gives them.
#[test]
fn a_scrape_returns_what_was_recorded_in_the_text_format() {
    let telemetry = prometheus_only(&LISTEN);
    let addr = telemetry.prometheus_addr().expect("listening");
    assert_ne!(addr.port(), 0, "the port the operating system chose");
    let meter = telemetry
        .meter_provider()
        .expect("a provider")
        .meter("scrape-test");
    let counter = meter
        .u64_counter("probe.count")
        .with_description("Counts probes")
        .build();
    counter.add(3, &[KeyValue::new("route", "/x")]);
    meter
        .f64_histogram("probe.duration")
        .with_unit("s")
        .build()
        .record(0.25, &[]);

    let got = scrape(addr, "GET /metrics HTTP/1.1");
    assert!(got.starts_with("HTTP/1.1 200"), "{got}");
    assert!(
        got.to_ascii_lowercase()
            .contains("content-type: text/plain; version=0.0.4; charset=utf-8"),
        "{got}"
    );
    assert!(got.contains("# TYPE probe_count_total counter"), "{got}");
    assert!(
        got.contains(r#"probe_count_total{route="/x",otel_scope_name="scrape-test"} 3"#),
        "{got}"
    );
    assert!(
        got.contains("# TYPE probe_duration_seconds histogram"),
        "{got}"
    );
    assert!(got.contains("probe_duration_seconds_count"), "{got}");
    assert!(
        got.contains("target_info{") && got.contains(r#"service_name="scrape-test""#),
        "{got}"
    );
    // A second scrape sees a later value: the endpoint reads the live
    // instruments, it does not serve a snapshot.
    counter.add(4, &[KeyValue::new("route", "/x")]);
    let again = scrape(addr, "GET /metrics HTTP/1.1");
    assert!(
        again.contains(r#"probe_count_total{route="/x",otel_scope_name="scrape-test"} 7"#),
        "{again}"
    );
}

#[test]
fn only_get_and_head_on_metrics_are_answered() {
    let telemetry = prometheus_only(&LISTEN);
    let addr = telemetry.prometheus_addr().expect("listening");
    assert!(scrape(addr, "GET / HTTP/1.1").starts_with("HTTP/1.1 404"));
    assert!(scrape(addr, "GET /metrics/extra HTTP/1.1").starts_with("HTTP/1.1 404"));
    let post = scrape(addr, "POST /metrics HTTP/1.1\r\nContent-Length: 0");
    assert!(post.starts_with("HTTP/1.1 405"), "{post}");
    assert!(
        post.to_ascii_lowercase().contains("allow: get, head"),
        "{post}"
    );
    let head = scrape(addr, "HEAD /metrics HTTP/1.1");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
}

#[test]
fn describe_says_where_the_scrape_goes_and_no_thread_is_needed_for_nothing_else() {
    let telemetry = prometheus_only(&LISTEN);
    let addr = telemetry.prometheus_addr().expect("listening");
    assert_eq!(
        telemetry.describe(),
        format!("telemetry: traces → off; metrics → prometheus http://{addr}/metrics; logs → off")
    );
}

#[test]
fn a_port_in_use_fails_the_build_naming_what_to_set() {
    let held = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = held.local_addr().expect("addr").port().to_string();
    let err = Telemetry::builder()
        .install_globally(false)
        .with_env(env(&[
            ("OTEL_TRACES_EXPORTER", "none"),
            ("OTEL_LOGS_EXPORTER", "none"),
            ("OTEL_METRICS_EXPORTER", "prometheus"),
            ("OTEL_EXPORTER_PROMETHEUS_HOST", "127.0.0.1"),
            ("OTEL_EXPORTER_PROMETHEUS_PORT", &port),
        ]))
        .build()
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("OTEL_EXPORTER_PROMETHEUS_PORT")
            && err.contains("cannot listen")
            && err.contains(&format!("127.0.0.1:{port}")),
        "{err}"
    );
}

/// A port that is not a number is noted and the default used, as the
/// specification has an SDK do with a value it cannot read (audit C-08).
/// The default is read directly: binding it would collide with another
/// process on a machine that already serves a scrape there.
#[test]
fn a_port_that_is_not_a_number_is_noted_and_the_default_used() {
    let notes = Notes::default();
    for bad in ["nine", "65536", "-1", "9464.5"] {
        let got = port(&env(&[("OTEL_EXPORTER_PROMETHEUS_PORT", bad)]), &notes);
        assert_eq!(got, 9464, "{bad:?}");
    }
    let lines = notes.take();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines[0].contains("OTEL_EXPORTER_PROMETHEUS_PORT") && lines[0].contains("nine"));
    assert!(lines[0].contains("9464 is used"), "{lines:?}");
}

#[test]
fn a_port_number_is_read_with_spaces_and_unset_is_the_default() {
    let notes = Notes::default();
    assert_eq!(
        port(&env(&[("OTEL_EXPORTER_PROMETHEUS_PORT", " 1234 ")]), &notes),
        1234
    );
    assert_eq!(
        port(&env(&[("OTEL_EXPORTER_PROMETHEUS_PORT", "0")]), &notes),
        0
    );
    assert_eq!(port(&env(&[]), &notes), 9464);
    assert_eq!(notes.take(), Vec::<String>::new());
}

/// `otlp,prometheus` is both: the export runtime serves the scrape and
/// sends to the collector.
#[test]
fn prometheus_can_run_beside_otlp() {
    let telemetry = prometheus_only(&[
        ("OTEL_METRICS_EXPORTER", "otlp,prometheus"),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:9"),
        ("OTEL_EXPORTER_PROMETHEUS_HOST", "127.0.0.1"),
        ("OTEL_EXPORTER_PROMETHEUS_PORT", "0"),
    ]);
    assert!(telemetry.prometheus_addr().is_some());
    let said = telemetry.describe();
    assert!(
        said.contains(
            "metrics → otlp http/protobuf http://127.0.0.1:9/v1/metrics + prometheus http://"
        ),
        "{said}"
    );
}

/// One request per connection: the response says `connection: close` and
/// the server closes, so a client cannot hold a connection open between
/// scrapes (audit C-16). Until 2026-11-04 the connection stayed open.
#[test]
fn a_connection_carries_one_request_and_is_closed() {
    let telemetry = prometheus_only(&LISTEN);
    let addr = telemetry.prometheus_addr().expect("listening");
    let mut stream = std::net::TcpStream::connect(addr).expect("connect");
    // A failure, not a hang, if the server keeps the connection open.
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(20)))
        .expect("timeout");
    stream
        .write_all(b"GET /metrics HTTP/1.1\r\nHost: t\r\n\r\n")
        .expect("write");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("the server closes the connection after one response");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(
        response.to_ascii_lowercase().contains("connection: close"),
        "{response}"
    );
}

/// At most `MAX_CONNECTIONS` connections are served at once: with that many
/// held open and silent, a scrape waits; when one of them goes, it is
/// answered.
#[test]
fn connections_past_the_cap_wait_for_a_slot() {
    let telemetry = prometheus_only(&LISTEN);
    let addr = telemetry.prometheus_addr().expect("listening");
    let mut held: Vec<std::net::TcpStream> = (0..MAX_CONNECTIONS)
        .map(|_| std::net::TcpStream::connect(addr).expect("connect"))
        .collect();
    let mut waiting = std::net::TcpStream::connect(addr).expect("the backlog accepts it");
    waiting
        .write_all(b"GET /metrics HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n")
        .expect("write");
    waiting
        .set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .expect("timeout");
    let mut byte = [0_u8; 1];
    let early = waiting.read(&mut byte);
    assert!(
        early.is_err(),
        "a connection past the cap was answered at once: {early:?}"
    );
    drop(held.pop());
    waiting
        .set_read_timeout(Some(std::time::Duration::from_secs(20)))
        .expect("timeout");
    let mut response = String::new();
    waiting.read_to_string(&mut response).expect("answered");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    drop(held);
}

/// Without the listener nothing is bound, and the handle renders what was
/// recorded, for the application to serve on its own port.
#[test]
fn without_the_listener_the_application_serves_the_text() {
    let telemetry = Telemetry::builder()
        .install_globally(false)
        .with_default_service_name("scrape-test")
        .without_prometheus_listener()
        .with_env(env(&[
            ("OTEL_TRACES_EXPORTER", "none"),
            ("OTEL_LOGS_EXPORTER", "none"),
            ("OTEL_METRICS_EXPORTER", "prometheus"),
            // Would collide if it were bound: the listener is not built.
            ("OTEL_EXPORTER_PROMETHEUS_PORT", "1"),
        ]))
        .build()
        .expect("builds");
    assert_eq!(telemetry.prometheus_addr(), None);
    assert_eq!(
        telemetry.describe(),
        "telemetry: traces → off; metrics → prometheus, served by the application (no \
         listener); logs → off"
    );
    let scrape = telemetry.prometheus().expect("a handle");
    assert_eq!(format!("{scrape:?}"), "PrometheusScrape { .. }");
    telemetry
        .meter_provider()
        .expect("a provider")
        .meter("scrape-test")
        .u64_counter("probe.count")
        .build()
        .add(2, &[]);
    let text = scrape.render().expect("renders");
    assert!(
        text.contains(r#"probe_count_total{otel_scope_name="scrape-test"} 2"#),
        "{text}"
    );
    assert_eq!(
        PrometheusScrape::CONTENT_TYPE,
        "text/plain; version=0.0.4; charset=utf-8"
    );
}

#[test]
fn without_prometheus_there_is_no_handle() {
    let telemetry = Telemetry::builder()
        .install_globally(false)
        .with_env(env(&[
            ("OTEL_TRACES_EXPORTER", "none"),
            ("OTEL_LOGS_EXPORTER", "none"),
            ("OTEL_METRICS_EXPORTER", "console"),
        ]))
        .build()
        .expect("builds");
    assert!(telemetry.prometheus().is_none());
}
