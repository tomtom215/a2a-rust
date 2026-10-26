// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Building one OTLP exporter per signal, on either transport.

use std::sync::Arc;
use std::time::Duration;

use hyper_util::client::proxy::matcher::Matcher;
use rustls_pki_types::pem::PemObject as _;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use super::TelemetryError;
use super::compression::OtlpCompression;
use super::config::{Env, OtlpProtocol, Signal};
use super::hints::Cause;
use super::http::ExportHttpClient;
use super::proxy;
use super::runtime::ExportRuntime;

/// Where one signal's exporter sends, and how.
pub(super) struct Target {
    pub(super) signal: Signal,
    pub(super) protocol: OtlpProtocol,
    /// The full URL for OTLP/HTTP, the base for gRPC; `None` leaves it to
    /// `opentelemetry-otlp`, which reads `OTEL_EXPORTER_OTLP_*_ENDPOINT` and
    /// falls back to `localhost` on the transport's port.
    pub(super) endpoint: Option<String>,
    /// Added to `OTEL_EXPORTER_OTLP_*_HEADERS`, which the exporter still
    /// reads; a variable naming the same header wins.
    pub(super) headers: Vec<(String, String)>,
    /// How export bodies are compressed: set in code, or named by
    /// `OTEL_EXPORTER_OTLP_*COMPRESSION`, which `compression::from_env`
    /// resolved. `None` sends them as they are.
    pub(super) compression: Option<OtlpCompression>,
    /// Which destinations go through which HTTP proxy (`proxy.rs`).
    pub(super) proxy: Arc<Matcher>,
    /// Where this exporter sends, as a person reads it: the endpoint with any
    /// login in the URL removed. For the startup line and for the message
    /// when an export is lost.
    pub(super) destination: String,
    /// What the HTTP client last failed with, kept for the loss report
    /// (`hints.rs`): the exporter crate discards it.
    pub(super) cause: Arc<Cause>,
    pub(super) timeout: Duration,
    /// Extra CA certificates, PEM, trusted alongside the Mozilla roots.
    pub(super) ca_pem: Vec<u8>,
    /// The client certificate presented for mutual TLS, if any.
    pub(super) client_identity: Option<ClientIdentity>,
}

/// `url` without a `user:password@` in its authority.
fn without_userinfo(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{scheme}://{host}{path}")
}

/// A client certificate chain (leaf first) and its private key, PEM.
#[derive(Clone)]
pub(super) struct ClientIdentity {
    pub(super) cert_pem: Vec<u8>,
    pub(super) key_pem: Vec<u8>,
}

impl ClientIdentity {
    /// The chain and key, parsed. A chain with no certificate is refused
    /// here; tonic's `Identity` would otherwise carry it to the first
    /// handshake, which would fail on every export.
    fn parse(
        &self,
    ) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TelemetryError> {
        let certs = CertificateDer::pem_slice_iter(&self.cert_pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TelemetryError::Tls(format!("the client certificate is not PEM: {e}")))?;
        if certs.is_empty() {
            return Err(TelemetryError::Tls(
                "the client certificate file contains no certificate".to_owned(),
            ));
        }
        let key = PrivateKeyDer::from_pem_slice(&self.key_pem)
            .map_err(|e| TelemetryError::Tls(format!("the client key is not PEM: {e}")))?;
        Ok((certs, key))
    }
}

impl Target {
    /// The endpoint this exporter will use, for display: the configured one,
    /// else what `OTEL_EXPORTER_OTLP_*ENDPOINT` names (the general variable
    /// gaining the signal's path on OTLP/HTTP, as `opentelemetry-otlp` does),
    /// else the transport's `localhost` default; userinfo removed.
    pub(super) fn destination_for(
        env: &Env,
        signal: Signal,
        protocol: OtlpProtocol,
        endpoint: Option<&str>,
    ) -> String {
        let url = match (endpoint, signal.otlp_var(env, "ENDPOINT")) {
            (Some(url), _) => url.to_owned(),
            (None, Some((var, value))) if var == "OTEL_EXPORTER_OTLP_ENDPOINT" => {
                Self::endpoint_from_base(&value, signal, protocol)
            }
            (None, Some((_, value))) => value,
            (None, None) if protocol.is_http() => {
                format!("http://localhost:4318/{}", signal.http_path())
            }
            (None, None) => "http://localhost:4317".to_owned(),
        };
        without_userinfo(&url)
    }

    /// The proxy `destination` goes through, if one applies, for display.
    pub(super) fn proxy_display(&self) -> Option<String> {
        let uri = self.destination.parse::<http::Uri>().ok()?;
        let via = proxy::intercept(&self.proxy, &uri)?;
        Some(via.uri().to_string().trim_end_matches('/').to_owned())
    }

    /// The endpoint for `signal` given a base URL, as
    /// `OTEL_EXPORTER_OTLP_ENDPOINT` is interpreted: OTLP/HTTP appends the
    /// signal's path, gRPC does not.
    pub(super) fn endpoint_from_base(base: &str, signal: Signal, protocol: OtlpProtocol) -> String {
        if protocol.is_http() {
            format!("{}/{}", base.trim_end_matches('/'), signal.http_path())
        } else {
            base.to_owned()
        }
    }

    /// The rustls configuration for OTLP/HTTP and for OTLP/gRPC over TLS:
    /// Mozilla's roots, plus the
    /// extra CAs, and the client certificate when there is one, with `ring` chosen explicitly so a process that links a
    /// second provider does not make rustls panic choosing one (the client
    /// crate's `tls.rs` records the same trap).
    fn rustls_config(&self) -> Result<rustls::ClientConfig, TelemetryError> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for cert in self.extra_cas()? {
            roots
                .add(cert)
                .map_err(|e| TelemetryError::Tls(format!("extra CA certificate refused: {e}")))?;
        }
        let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| TelemetryError::Tls(e.to_string()))?
        .with_root_certificates(roots);
        match &self.client_identity {
            None => Ok(builder.with_no_client_auth()),
            Some(identity) => {
                let (certs, key) = identity.parse()?;
                builder.with_client_auth_cert(certs, key).map_err(|e| {
                    TelemetryError::Tls(format!(
                        "the client certificate and key are not usable: {e}"
                    ))
                })
            }
        }
    }

    fn extra_cas(&self) -> Result<Vec<CertificateDer<'static>>, TelemetryError> {
        if self.ca_pem.is_empty() {
            return Ok(Vec::new());
        }
        let certs = CertificateDer::pem_slice_iter(&self.ca_pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TelemetryError::Tls(format!("extra CA certificates are not PEM: {e}")))?;
        if certs.is_empty() {
            return Err(TelemetryError::Tls(
                "extra CA certificates were given but contain no certificate".to_owned(),
            ));
        }
        Ok(certs)
    }

    /// The OTLP/gRPC endpoint: the configured or environment one, else
    /// the conventional plaintext `http://localhost:4317`.
    fn grpc_endpoint(&self) -> String {
        self.endpoint
            .clone()
            .unwrap_or_else(|| "http://localhost:4317".to_owned())
    }

    /// `inner`, with this crate's TLS over it, offering ALPN `h2`, dialled as
    /// `https` whatever scheme tonic hands it ([`DialsTls`]).
    fn dials_tls<T>(&self, inner: T) -> Result<DialsTls<T>, TelemetryError> {
        Ok(DialsTls(
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_tls_config(self.rustls_config()?)
                .https_only()
                .enable_http2()
                .wrap_connector(inner),
        ))
    }

    /// Whether `endpoint` is reached through an HTTP proxy (`proxy.rs`).
    fn proxied(&self, endpoint: &str) -> bool {
        endpoint
            .parse::<http::Uri>()
            .is_ok_and(|uri| proxy::intercept(&self.proxy, &uri).is_some())
    }

    /// A tonic channel to an `https://` collector over this crate's rustls
    /// stack: the roots, extra CAs and client certificate
    /// [`rustls_config`](Self::rustls_config) builds, offering ALPN `h2`
    /// (`enable_http2`, which refuses a config that sets ALPN itself).
    ///
    /// tonic is given the endpoint as `http://` and [`DialsTls`] dials it as
    /// `https`. With any tonic TLS feature compiled in, as an application's
    /// own tonic or the client crate's gRPC TLS can turn on, tonic refuses an
    /// `https://` URI it holds no TLS config for ("Connecting to HTTPS without
    /// TLS enabled", tonic 0.14.6 `connector.rs`), whoever dials it. TLS is
    /// not optional on this path: the connector is `https_only`. The channel
    /// connects on first use.
    ///
    /// Through an HTTP proxy the connector first opens a `CONNECT` tunnel to
    /// the collector, and the TLS above runs inside it, so the collector's
    /// certificate is still checked; a plaintext `http://` collector is
    /// tunnelled too, since HTTP/2 cannot be forwarded as a plain request.
    fn grpc_channel(&self, endpoint: &str) -> Result<tonic::transport::Channel, TelemetryError> {
        use hyper_util::client::legacy::connect::HttpConnector;
        let mut tcp = HttpConnector::new();
        tcp.enforce_http(false);
        let tunnel = endpoint
            .parse::<http::Uri>()
            .ok()
            .and_then(|uri| proxy::intercept(&self.proxy, &uri))
            .map(|via| proxy::tunnel(&via, tcp.clone()));
        let https = endpoint.starts_with("https://");
        let plain = format!(
            "http://{}",
            endpoint
                .trim_start_matches("https://")
                .trim_start_matches("http://")
        );
        let channel = tonic::transport::Endpoint::from_shared(plain)
            .map_err(|e| TelemetryError::config("OTLP endpoint", endpoint, e.to_string()))?
            .timeout(self.timeout);
        Ok(match (https, tunnel) {
            (true, Some(tunnel)) => channel.connect_with_connector_lazy(self.dials_tls(tunnel)?),
            (true, None) => channel.connect_with_connector_lazy(self.dials_tls(tcp)?),
            (false, Some(tunnel)) => channel.connect_with_connector_lazy(tunnel),
            (false, None) => channel.connect_lazy(),
        })
    }

    fn grpc_metadata(
        &self,
    ) -> Result<opentelemetry_otlp::tonic_types::metadata::MetadataMap, TelemetryError> {
        let mut map = http::HeaderMap::new();
        for (k, v) in &self.headers {
            let name = http::HeaderName::from_bytes(k.as_bytes()).map_err(|_| {
                TelemetryError::config("OTLP header name", k, "not a valid header name")
            })?;
            let value = http::HeaderValue::from_str(v).map_err(|_| {
                TelemetryError::config(
                    "OTLP header value",
                    k,
                    "not a valid header value for this header",
                )
            })?;
            map.insert(name, value);
        }
        Ok(opentelemetry_otlp::tonic_types::metadata::MetadataMap::from_headers(map))
    }
}

/// The OTLP/gRPC connector: dials every URI it is given as `https`, through
/// a TLS connector. See [`Target::grpc_channel`].
#[derive(Clone)]
struct DialsTls<T>(hyper_rustls::HttpsConnector<T>);

impl<T> tower_service::Service<http::Uri> for DialsTls<T>
where
    hyper_rustls::HttpsConnector<T>: tower_service::Service<http::Uri>,
{
    type Response =
        <hyper_rustls::HttpsConnector<T> as tower_service::Service<http::Uri>>::Response;
    type Error = <hyper_rustls::HttpsConnector<T> as tower_service::Service<http::Uri>>::Error;
    type Future = <hyper_rustls::HttpsConnector<T> as tower_service::Service<http::Uri>>::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, uri: http::Uri) -> Self::Future {
        let mut parts = uri.into_parts();
        parts.scheme = Some(http::uri::Scheme::HTTPS);
        // Rebuilt from a URI that parsed, with only the scheme changed; the
        // fallback cannot be reached, and would be refused as not https.
        let uri = http::Uri::from_parts(parts).unwrap_or_default();
        self.0.call(uri)
    }
}

/// Builds `$exporter` (`SpanExporter`, `MetricExporter` or `LogExporter`) for
/// `$target`. The three builders share their configuration traits but no
/// common type, so this is a macro rather than a generic function.
macro_rules! build_exporter {
    ($exporter:ty, $target:expr, $runtime:expr) => {{
        use opentelemetry_otlp::{
            WithExportConfig as _, WithHttpConfig as _, WithTonicConfig as _,
        };
        let target: &Target = $target;
        let runtime: &ExportRuntime = $runtime;
        match target.protocol {
            OtlpProtocol::Grpc => {
                // tonic spawns the channel's worker while the exporter is
                // built; inside this guard it spawns onto the export runtime.
                let _entered = runtime.handle().enter();
                let endpoint = target.grpc_endpoint();
                let mut builder = <$exporter>::builder()
                    .with_tonic()
                    .with_timeout(target.timeout);
                builder = if endpoint.starts_with("https://") || target.proxied(&endpoint) {
                    builder.with_channel(target.grpc_channel(&endpoint)?)
                } else {
                    builder.with_endpoint(endpoint)
                };
                if !target.headers.is_empty() {
                    builder = builder.with_metadata(target.grpc_metadata()?);
                }
                if let Some(compression) = target.compression {
                    builder = builder.with_compression(compression.to_upstream());
                }
                builder.build()
            }
            OtlpProtocol::HttpProtobuf => http_exporter!($exporter, target, runtime, HttpBinary),
            #[cfg(feature = "otlp-json")]
            OtlpProtocol::HttpJson => http_exporter!($exporter, target, runtime, HttpJson),
        }
        .map_err(|e| TelemetryError::Exporter {
            signal: target.signal.name(),
            source: e,
        })
    }};
}

/// Builds `$exporter` for OTLP/HTTP with `$encoding`
/// (`opentelemetry_otlp::Protocol`'s `HttpBinary` or `HttpJson`).
macro_rules! http_exporter {
    ($exporter:ty, $target:expr, $runtime:expr, $encoding:ident) => {{
        let target: &Target = $target;
        let runtime: &ExportRuntime = $runtime;
        let client = ExportHttpClient::new(
            runtime.handle().clone(),
            &target.rustls_config()?,
            target.timeout,
            Arc::clone(&target.proxy),
            Arc::clone(&target.cause),
        );
        let mut builder = <$exporter>::builder()
            .with_http()
            .with_protocol(opentelemetry_otlp::Protocol::$encoding)
            .with_http_client(client)
            .with_timeout(target.timeout);
        if let Some(endpoint) = &target.endpoint {
            builder = builder.with_endpoint(endpoint.clone());
        }
        if !target.headers.is_empty() {
            builder = builder.with_headers(target.headers.iter().cloned().collect());
        }
        if let Some(compression) = target.compression {
            builder = builder.with_compression(compression.to_upstream());
        }
        builder.build()
    }};
}

pub(super) fn spans(
    target: &Target,
    runtime: &ExportRuntime,
) -> Result<opentelemetry_otlp::SpanExporter, TelemetryError> {
    build_exporter!(opentelemetry_otlp::SpanExporter, target, runtime)
}

pub(super) fn metrics(
    target: &Target,
    runtime: &ExportRuntime,
) -> Result<opentelemetry_otlp::MetricExporter, TelemetryError> {
    build_exporter!(opentelemetry_otlp::MetricExporter, target, runtime)
}

pub(super) fn logs(
    target: &Target,
    runtime: &ExportRuntime,
) -> Result<opentelemetry_otlp::LogExporter, TelemetryError> {
    build_exporter!(opentelemetry_otlp::LogExporter, target, runtime)
}

#[cfg(test)]
mod tests {
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
        let own = |k: &str| {
            (k == "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT").then(|| "http://own:1/x".to_owned())
        };
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
}
