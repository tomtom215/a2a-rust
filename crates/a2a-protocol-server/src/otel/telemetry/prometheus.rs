// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `OTEL_METRICS_EXPORTER=prometheus`: the metrics, served at `/metrics` in
//! the Prometheus text format for a Prometheus that scrapes this process,
//! with no collector in between.
//!
//! `OTEL_METRICS_EXPORTER=prometheus` used to be refused, which left a
//! Prometheus-scrape deployment without a collector no way to get these
//! metrics out. The names and units are `opentelemetry-prometheus`'s — the
//! translation the book's "What these become in Prometheus" table was
//! measured through — so a dashboard built for the OTLP path reads the same
//! series here.
//!
//! The listener is plain HTTP, unauthenticated, and bound where
//! `OTEL_EXPORTER_PROMETHEUS_HOST` says: `localhost` by default (the
//! specification's default), so a scraper on another machine needs
//! `0.0.0.0` or an address, and then anyone who can reach the port can read
//! the metrics. It runs on the export runtime and stops with it.

use std::convert::Infallible;
use std::net::{SocketAddr, TcpListener};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use opentelemetry_prometheus::PrometheusExporter;
use prometheus::{Registry, TextEncoder};

use super::TelemetryError;
use super::config::Env;
use super::runtime::ExportRuntime;

/// The specification's default port for the Prometheus exporter.
const DEFAULT_PORT: u16 = 9464;

/// A scraper must send its request line and headers within this.
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);

/// The reader to give the meter provider, and where it is served.
pub(super) struct Started {
    pub(super) reader: PrometheusExporter,
    pub(super) addr: SocketAddr,
}

/// Binds `OTEL_EXPORTER_PROMETHEUS_HOST`:`_PORT` and starts serving.
///
/// # Errors
///
/// A port that is not a number, an address that cannot be listened on (the
/// port in use, say), or an exporter that cannot be built — each naming what
/// to change.
pub(super) fn start(env: &Env, runtime: &ExportRuntime) -> Result<Started, TelemetryError> {
    const HOST: &str = "OTEL_EXPORTER_PROMETHEUS_HOST";
    const PORT: &str = "OTEL_EXPORTER_PROMETHEUS_PORT";
    let host = env(HOST).unwrap_or_else(|| "localhost".to_owned());
    let port = match env(PORT) {
        None => DEFAULT_PORT,
        Some(value) => value
            .trim()
            .parse::<u16>()
            .map_err(|_| TelemetryError::config(PORT, &value, "expected a port number"))?,
    };
    // Bound here, not in the serving task, so a port in use fails the build
    // — where it is seen — and not the first scrape.
    let listener = TcpListener::bind((host.as_str(), port)).map_err(|e| {
        TelemetryError::config(
            PORT,
            &format!("{host}:{port}"),
            format!("cannot listen there: {e}; set {HOST} and {PORT}"),
        )
    })?;
    listener
        .set_nonblocking(true)
        .map_err(|e| TelemetryError::config(PORT, &port.to_string(), e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| TelemetryError::config(PORT, &port.to_string(), e.to_string()))?;
    let registry = Registry::new();
    let reader = opentelemetry_prometheus::exporter()
        .with_registry(registry.clone())
        .build()
        .map_err(|e| {
            TelemetryError::config("OTEL_METRICS_EXPORTER", "prometheus", e.to_string())
        })?;
    drop(runtime.handle().spawn(serve(listener, registry)));
    Ok(Started { reader, addr })
}

async fn serve(listener: TcpListener, registry: Registry) {
    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
        return;
    };
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            // Out of descriptors, or the like: wait rather than spin.
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        let registry = registry.clone();
        drop(tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req: Request<Incoming>| {
                let response = respond(&registry, &req);
                async move { Ok::<_, Infallible>(response) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_TIMEOUT)
                .serve_connection(TokioIo::new(stream), service)
                .await;
        }));
    }
}

fn plain(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from_static(body.as_bytes())));
    *response.status_mut() = status;
    response
}

/// `/metrics` answers `GET` and `HEAD` with the registry in the text format;
/// anything else is refused for what it is.
fn respond(registry: &Registry, req: &Request<Incoming>) -> Response<Full<Bytes>> {
    if req.uri().path() != "/metrics" {
        return plain(StatusCode::NOT_FOUND, "the metrics are at /metrics\n");
    }
    if req.method() != Method::GET && req.method() != Method::HEAD {
        let mut refused = plain(StatusCode::METHOD_NOT_ALLOWED, "GET /metrics\n");
        refused.headers_mut().insert(
            hyper::header::ALLOW,
            hyper::header::HeaderValue::from_static("GET, HEAD"),
        );
        return refused;
    }
    TextEncoder::new()
        .encode_to_string(&registry.gather())
        .map_or_else(
            |_| {
                plain(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "could not encode the metrics\n",
                )
            },
            |text| {
                let mut response = Response::new(Full::new(Bytes::from(text)));
                response.headers_mut().insert(
                    hyper::header::CONTENT_TYPE,
                    hyper::header::HeaderValue::from_static(
                        "text/plain; version=0.0.4; charset=utf-8",
                    ),
                );
                response
            },
        )
}

#[cfg(test)]
mod tests {
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
            format!(
                "telemetry: traces → off; metrics → prometheus http://{addr}/metrics; logs → off"
            )
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

    #[test]
    fn a_port_that_is_not_a_number_is_refused() {
        let err = Telemetry::builder()
            .install_globally(false)
            .with_env(env(&[
                ("OTEL_TRACES_EXPORTER", "none"),
                ("OTEL_LOGS_EXPORTER", "none"),
                ("OTEL_METRICS_EXPORTER", "prometheus"),
                ("OTEL_EXPORTER_PROMETHEUS_PORT", "nine"),
            ]))
            .build()
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("OTEL_EXPORTER_PROMETHEUS_PORT") && err.contains("nine"),
            "{err}"
        );
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
}
