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
//!
//! It serves at most [`MAX_CONNECTIONS`] connections at once, one request
//! each, within [`EXCHANGE_TIMEOUT`]: until 2026-11-04 it took every
//! connection offered and kept each open as long as the client liked, so a
//! client that opened connections and sent nothing held the port (audit
//! C-16). To serve `/metrics` on the application's own port instead, behind
//! its authentication and connection limits, take
//! [`Telemetry::prometheus`](super::Telemetry::prometheus) and build with
//! `TelemetryBuilder::without_prometheus_listener` (ADR 0038 §3).

use std::convert::Infallible;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use opentelemetry_prometheus::PrometheusExporter;
use prometheus::{Registry, TextEncoder};

use super::TelemetryError;
use super::config::{Env, Notes};
use super::runtime::ExportRuntime;

/// The specification's default port for the Prometheus exporter.
const DEFAULT_PORT: u16 = 9464;

/// A scraper must send its request line and headers within this.
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);

/// The most scrape connections served at once; a further one waits in the
/// listen backlog until one ends. A Prometheus server uses one per target.
pub(super) const MAX_CONNECTIONS: usize = 16;

/// A connection carries one request and its response, and is closed after
/// this however far it got.
pub(super) const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(30);

/// The reader to give the meter provider, where it is served (none without
/// the listener), and the handle that renders the same text.
pub(super) struct Started {
    pub(super) reader: PrometheusExporter,
    pub(super) addr: Option<SocketAddr>,
    pub(super) scrape: PrometheusScrape,
}

/// The metrics in the Prometheus text format, for an application to serve.
///
/// It serves them at `/metrics` on a port of its own — behind its own
/// authentication and connection limits — instead of, or beside, the
/// listener on `OTEL_EXPORTER_PROMETHEUS_PORT`. From
/// [`Telemetry::prometheus`](super::Telemetry::prometheus).
#[derive(Clone)]
pub struct PrometheusScrape {
    registry: Registry,
}

impl std::fmt::Debug for PrometheusScrape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrometheusScrape").finish_non_exhaustive()
    }
}

impl PrometheusScrape {
    /// The `Content-Type` to send [`render`](Self::render)'s text with.
    pub const CONTENT_TYPE: &'static str = "text/plain; version=0.0.4; charset=utf-8";

    /// Everything recorded so far, in the text format: what a scrape of the
    /// listener returns.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::Export`] when the metrics cannot be encoded.
    pub fn render(&self) -> Result<String, TelemetryError> {
        TextEncoder::new()
            .encode_to_string(&self.registry.gather())
            .map_err(|e| TelemetryError::Export(vec![format!("metrics: {e}")]))
    }
}

/// `OTEL_EXPORTER_PROMETHEUS_PORT`: the default when unset, and when it is not
/// a port number (noted).
fn port(env: &Env, notes: &Notes) -> u16 {
    const PORT: &str = "OTEL_EXPORTER_PROMETHEUS_PORT";
    let Some(value) = env(PORT) else {
        return DEFAULT_PORT;
    };
    value.trim().parse::<u16>().unwrap_or_else(|_| {
        notes.ignored(
            PORT,
            &value,
            "is not a port number",
            &format!("{DEFAULT_PORT} is used"),
        );
        DEFAULT_PORT
    })
}

/// Binds `OTEL_EXPORTER_PROMETHEUS_HOST`:`_PORT` and starts serving, when
/// `listen`; otherwise only builds the reader and the [`PrometheusScrape`].
///
/// A port that is not a number is noted and the default used (audit C-08).
///
/// # Errors
///
/// An address that cannot be listened on (the port in use, say) or an exporter
/// that cannot be built, naming what to change.
pub(super) fn start(
    env: &Env,
    runtime: &ExportRuntime,
    notes: &Notes,
    listen: bool,
) -> Result<Started, TelemetryError> {
    const HOST: &str = "OTEL_EXPORTER_PROMETHEUS_HOST";
    const PORT: &str = "OTEL_EXPORTER_PROMETHEUS_PORT";
    let registry = Registry::new();
    let reader = opentelemetry_prometheus::exporter()
        .with_registry(registry.clone())
        .build()
        .map_err(|e| {
            TelemetryError::config("OTEL_METRICS_EXPORTER", "prometheus", e.to_string())
        })?;
    let scrape = PrometheusScrape {
        registry: registry.clone(),
    };
    if !listen {
        return Ok(Started {
            reader,
            addr: None,
            scrape,
        });
    }
    let host = env(HOST).unwrap_or_else(|| "localhost".to_owned());
    let port = port(env, notes);
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
    drop(runtime.handle().spawn(serve(listener, registry)));
    Ok(Started {
        reader,
        addr: Some(addr),
        scrape,
    })
}

async fn serve(listener: TcpListener, registry: Registry) {
    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
        return;
    };
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        // A slot first, then the connection: past the cap, connections wait
        // in the backlog rather than being taken and held.
        let Ok(slot) = Arc::clone(&slots).acquire_owned().await else {
            return;
        };
        let Ok((stream, _)) = listener.accept().await else {
            // Out of descriptors, or the like: wait rather than spin.
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        let registry = registry.clone();
        drop(tokio::spawn(async move {
            let _slot = slot;
            let service = hyper::service::service_fn(move |req: Request<Incoming>| {
                let response = respond(&registry, &req);
                async move { Ok::<_, Infallible>(response) }
            });
            let connection = hyper::server::conn::http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_TIMEOUT)
                .keep_alive(false)
                .serve_connection(TokioIo::new(stream), service);
            let _ = tokio::time::timeout(EXCHANGE_TIMEOUT, connection).await;
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
                    hyper::header::HeaderValue::from_static(PrometheusScrape::CONTENT_TYPE),
                );
                response
            },
        )
}

#[cfg(test)]
mod tests;
