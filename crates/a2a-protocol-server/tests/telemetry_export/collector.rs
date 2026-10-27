// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Stand-in OTLP collectors: plaintext HTTP, HTTPS behind a private CA, and
//! gRPC, each over TLS optionally requiring a client certificate the CA
//! signed. OTLP/HTTP bodies may be protobuf or JSON. Each records what it receives and decodes it as a real collector
//! would, so a test asserts on spans, metrics and log records rather than on
//! bytes having arrived.
//!
//! Each runs on a thread and runtime of its own. A collector on the test's
//! runtime would be starved while the test blocks in `Telemetry::shutdown`,
//! and would hide exactly the deadlock the export runtime exists to prevent.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bytes::Bytes;
use http_body_util::{BodyExt as _, Full};
use opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::{
    LogsService, LogsServiceServer,
};
use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_server::{
    MetricsService, MetricsServiceServer,
};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::KeyValue;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::trace::v1::Span;
use prost::Message as _;
use tokio::sync::oneshot;

use super::body;
use super::certs::TestCerts;

/// One export request as the collector saw it. gRPC requests are recorded
/// under the OTLP/HTTP path of their signal, re-encoded, so one decoder
/// serves both transports.
#[derive(Clone, Debug)]
pub struct Received {
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

type Log = Arc<Mutex<Vec<Received>>>;

pub struct Collector {
    pub addr: SocketAddr,
    received: Log,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Collector {
    fn start<F, Fut>(serve: F) -> Self
    where
        F: FnOnce(tokio::net::TcpListener, Log, oneshot::Receiver<()>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()>,
    {
        let received: Log = Arc::default();
        let (stop, stopped) = oneshot::channel();
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let log = Arc::clone(&received);
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("collector runtime");
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind");
                addr_tx
                    .send(listener.local_addr().expect("addr"))
                    .expect("send addr");
                serve(listener, log, stopped).await;
            });
        });
        Self {
            addr: addr_rx.recv().expect("collector address"),
            received,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    /// OTLP/HTTP, plaintext.
    pub fn http() -> Self {
        Self::http_answering(200)
    }

    /// OTLP/HTTP, plaintext, answering every export with `status`.
    pub fn http_answering(status: u16) -> Self {
        Self::http_answering_in_turn(vec![status])
    }

    /// OTLP/HTTP, plaintext, answering the n-th export request with
    /// `statuses[n]`, and every request after the last with the last.
    pub fn http_answering_in_turn(statuses: Vec<u16>) -> Self {
        Self::start(move |listener, log, stopped| {
            serve_http(listener, log, None, statuses, stopped)
        })
    }

    /// OTLP/HTTP over TLS, presenting the `localhost` certificate.
    pub fn https(certs: &TestCerts) -> Self {
        Self::https_with(certs, false)
    }

    /// OTLP/HTTP over TLS, refusing a client without a certificate the
    /// private CA signed.
    pub fn https_requiring_client_cert(certs: &TestCerts) -> Self {
        Self::https_with(certs, true)
    }

    fn https_with(certs: &TestCerts, client_auth: bool) -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = rustls::ServerConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .expect("protocol versions");
        let builder = if client_auth {
            let mut roots = rustls::RootCertStore::empty();
            roots.add(certs.ca_cert.clone()).expect("CA");
            let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
                Arc::new(roots),
                provider,
            )
            .build()
            .expect("client verifier");
            builder.with_client_cert_verifier(verifier)
        } else {
            builder.with_no_client_auth()
        };
        let config = builder
            .with_single_cert(
                vec![certs.server_cert.clone()],
                certs.server_key.clone_key(),
            )
            .expect("server cert");
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        Self::start(move |listener, log, stopped| {
            serve_http(listener, log, Some(acceptor), vec![200], stopped)
        })
    }

    /// OTLP/gRPC, plaintext.
    pub fn grpc() -> Self {
        Self::grpc_with(None)
    }

    /// OTLP/gRPC over TLS, presenting the `localhost` certificate.
    pub fn grpcs(certs: &TestCerts) -> Self {
        Self::grpcs_with(certs, false)
    }

    /// OTLP/gRPC over TLS, refusing a client without a certificate the
    /// private CA signed.
    pub fn grpcs_requiring_client_cert(certs: &TestCerts) -> Self {
        Self::grpcs_with(certs, true)
    }

    fn grpcs_with(certs: &TestCerts, client_auth: bool) -> Self {
        // tonic's *server* builds its rustls config from the process default
        // provider (`server/service/tls.rs`, tonic 0.14.6), and this test
        // binary links both `ring` and `aws-lc-rs`, so rustls refuses to
        // choose. The client side needs nothing: tonic picks `ring` itself
        // when no default is installed. Installing one twice is harmless.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut tls = tonic::transport::ServerTlsConfig::new().identity(
            tonic::transport::Identity::from_pem(&certs.server_cert_pem, &certs.server_key_pem),
        );
        if client_auth {
            tls = tls.client_ca_root(tonic::transport::Certificate::from_pem(&certs.ca_pem));
        }
        Self::grpc_with(Some(tls))
    }

    fn grpc_with(tls: Option<tonic::transport::ServerTlsConfig>) -> Self {
        Self::start(move |listener, log, stopped| async move {
            let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
            let svc = Grpc(log);
            let mut server = tonic::transport::Server::builder();
            if let Some(tls) = tls {
                server = server.tls_config(tls).expect("server TLS");
            }
            let gzip = tonic::codec::CompressionEncoding::Gzip;
            let _ = server
                .add_service(TraceServiceServer::new(svc.clone()).accept_compressed(gzip))
                .add_service(MetricsServiceServer::new(svc.clone()).accept_compressed(gzip))
                .add_service(LogsServiceServer::new(svc).accept_compressed(gzip))
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = stopped.await;
                })
                .await;
        })
    }

    pub fn http_base(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn https_base(&self) -> String {
        format!("https://localhost:{}", self.addr.port())
    }

    pub fn received(&self) -> Vec<Received> {
        self.received.lock().expect("lock").clone()
    }

    pub fn paths(&self) -> Vec<String> {
        self.received().into_iter().map(|r| r.path).collect()
    }

    /// Every span received, decoded, with its resource's attributes.
    pub fn spans(&self) -> Vec<(Vec<KeyValue>, Span)> {
        let mut out = Vec::new();
        for r in self
            .received()
            .into_iter()
            .filter(|r| r.path.ends_with("/v1/traces"))
        {
            let req = ExportTraceServiceRequest::decode(&*r.body).expect("an OTLP trace request");
            for rs in req.resource_spans {
                let resource = rs.resource.map(|r| r.attributes).unwrap_or_default();
                for ss in rs.scope_spans {
                    for span in ss.spans {
                        out.push((resource.clone(), span));
                    }
                }
            }
        }
        out
    }

    /// Every log record's body, as a string.
    pub fn log_bodies(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in self
            .received()
            .into_iter()
            .filter(|r| r.path.ends_with("/v1/logs"))
        {
            let req = ExportLogsServiceRequest::decode(&*r.body).expect("an OTLP logs request");
            for rl in req.resource_logs {
                for sl in rl.scope_logs {
                    for rec in sl.log_records {
                        if let Some(Value::StringValue(s)) = rec.body.and_then(|b| b.value) {
                            out.push(s);
                        }
                    }
                }
            }
        }
        out
    }

    /// Every metric name received.
    pub fn metric_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in self
            .received()
            .into_iter()
            .filter(|r| r.path.ends_with("/v1/metrics"))
        {
            let req =
                ExportMetricsServiceRequest::decode(&*r.body).expect("an OTLP metrics request");
            for rm in req.resource_metrics {
                for sm in rm.scope_metrics {
                    out.extend(sm.metrics.into_iter().map(|m| m.name));
                }
            }
        }
        out
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The string value of attribute `key`, if present.
pub fn attr(attrs: &[KeyValue], key: &str) -> Option<String> {
    attrs.iter().find(|kv| kv.key == key).and_then(|kv| {
        match kv.value.as_ref()?.value.as_ref()? {
            Value::StringValue(s) => Some(s.clone()),
            Value::IntValue(i) => Some(i.to_string()),
            Value::BoolValue(b) => Some(b.to_string()),
            other => Some(format!("{other:?}")),
        }
    })
}

/// A collector that is never reached: a listener that accepts each
/// connection and closes it at once, held for the test's lifetime so no other
/// test can take its port. It fails an export as fast on every OS. A closed
/// port does not: Windows retries the connection for about 2 s, so
/// `opentelemetry-otlp`'s four attempts outlast the batch processor's 5 s
/// shutdown wait and the loss is reported as that timeout instead (failed on
/// Windows on 2026-10-12). Port 1 was used before that; a port bound and
/// released was reused by another test's server under full-suite load
/// (observed 2026-10-11), and the export then succeeded.
pub fn unreachable() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            drop(stream);
        }
    });
    format!("http://{addr}")
}

async fn serve_http(
    listener: tokio::net::TcpListener,
    log: Log,
    tls: Option<tokio_rustls::TlsAcceptor>,
    statuses: Vec<u16>,
    mut stopped: oneshot::Receiver<()>,
) {
    let statuses = Arc::new(statuses);
    loop {
        let (stream, _) = tokio::select! {
            accepted = listener.accept() => match accepted { Ok(a) => a, Err(_) => return },
            _ = &mut stopped => return,
        };
        let log = Arc::clone(&log);
        let tls = tls.clone();
        let statuses = Arc::clone(&statuses);
        tokio::spawn(async move {
            let service =
                hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let log = Arc::clone(&log);
                    let statuses = Arc::clone(&statuses);
                    async move {
                        let path = req.uri().path().to_owned();
                        let headers = req
                            .headers()
                            .iter()
                            .map(|(k, v)| {
                                (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned())
                            })
                            .collect();
                        let json = body::says_json(&headers);
                        let gzipped =
                            headers.get("content-encoding").map(String::as_str) == Some("gzip");
                        let body = req
                            .into_body()
                            .collect()
                            .await
                            .map(|b| b.to_bytes().to_vec())
                            .unwrap_or_default();
                        let body = if gzipped { body::gunzip(&body) } else { body };
                        let body = if json {
                            body::json_to_protobuf(&path, &body)
                        } else {
                            body
                        };
                        let status = {
                            let mut log = log.lock().expect("lock");
                            log.push(Received {
                                path,
                                headers,
                                body,
                            });
                            statuses[(log.len() - 1).min(statuses.len() - 1)]
                        };
                        Ok::<_, std::convert::Infallible>(
                            hyper::Response::builder()
                                .status(status)
                                .header("content-type", "application/x-protobuf")
                                // A real collector's success reply: an
                                // `Export*ServiceResponse` with an empty
                                // `partial_success` (field 1). Not empty,
                                // so the exporter's response-size limit is
                                // exercised on every export.
                                .body(Full::new(Bytes::from_static(b"\x0a\x00")))
                                .expect("response"),
                        )
                    }
                });
            let builder =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
            match tls {
                None => {
                    let _ = builder
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                        .await;
                }
                Some(acceptor) => {
                    // A client that does not trust the CA fails the handshake
                    // here, and nothing is recorded — which is what the
                    // counter-test asserts.
                    if let Ok(tls_stream) = acceptor.accept(stream).await {
                        let _ = builder
                            .serve_connection(hyper_util::rt::TokioIo::new(tls_stream), service)
                            .await;
                    }
                }
            }
        });
    }
}

#[derive(Clone)]
struct Grpc(Log);

impl Grpc {
    fn record<T: prost::Message>(&self, path: &str, req: tonic::Request<T>) {
        let headers = req
            .metadata()
            .clone()
            .into_headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
            .collect();
        let body = req.into_inner().encode_to_vec();
        self.0.lock().expect("lock").push(Received {
            path: path.to_owned(),
            headers,
            body,
        });
    }
}

#[tonic::async_trait]
impl TraceService for Grpc {
    async fn export(
        &self,
        req: tonic::Request<ExportTraceServiceRequest>,
    ) -> Result<tonic::Response<ExportTraceServiceResponse>, tonic::Status> {
        self.record("/v1/traces", req);
        Ok(tonic::Response::new(ExportTraceServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl MetricsService for Grpc {
    async fn export(
        &self,
        req: tonic::Request<ExportMetricsServiceRequest>,
    ) -> Result<tonic::Response<ExportMetricsServiceResponse>, tonic::Status> {
        self.record("/v1/metrics", req);
        Ok(tonic::Response::new(ExportMetricsServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl LogsService for Grpc {
    async fn export(
        &self,
        req: tonic::Request<ExportLogsServiceRequest>,
    ) -> Result<tonic::Response<ExportLogsServiceResponse>, tonic::Status> {
        self.record("/v1/logs", req);
        Ok(tonic::Response::new(ExportLogsServiceResponse::default()))
    }
}
