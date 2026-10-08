// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Building one OTLP exporter per signal, on either transport.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use opentelemetry_otlp::tonic_types::transport::{Certificate, ClientTlsConfig};
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject as _;

use super::TelemetryError;
use super::config::{OtlpProtocol, Signal};
use super::http::ExportHttpClient;
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
    pub(super) timeout: Duration,
    /// Extra CA certificates, PEM, trusted alongside the Mozilla roots.
    pub(super) ca_pem: Vec<u8>,
    /// Counts this signal's failed OTLP/HTTP exports; see `http.rs`.
    pub(super) failures: Arc<AtomicU64>,
}

impl Target {
    /// The endpoint for `signal` given a base URL, as
    /// `OTEL_EXPORTER_OTLP_ENDPOINT` is interpreted: OTLP/HTTP appends the
    /// signal's path, gRPC does not.
    pub(super) fn endpoint_from_base(base: &str, signal: Signal, protocol: OtlpProtocol) -> String {
        match protocol {
            OtlpProtocol::Grpc => base.to_owned(),
            OtlpProtocol::HttpProtobuf => {
                format!("{}/{}", base.trim_end_matches('/'), signal.http_path())
            }
        }
    }

    /// The rustls configuration for OTLP/HTTP: Mozilla's roots, plus the
    /// extra CAs, with `ring` chosen explicitly so a process that links a
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
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| TelemetryError::Tls(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Ok(config)
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

    /// The tonic TLS configuration. `opentelemetry-otlp` builds an empty
    /// `ClientTlsConfig` for an `https://` endpoint when given none, and
    /// tonic enables no roots for an empty one (`with_webpki_roots` defaults
    /// to off, tonic 0.14.6), so every handshake would fail. TLS applies only
    /// to an `https://` endpoint; a plaintext one stays plaintext.
    fn grpc_tls(&self) -> Result<ClientTlsConfig, TelemetryError> {
        let mut tls = ClientTlsConfig::new().with_webpki_roots();
        if !self.ca_pem.is_empty() {
            self.extra_cas()?;
            tls = tls.ca_certificate(Certificate::from_pem(&self.ca_pem));
        }
        Ok(tls)
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
                let mut builder = <$exporter>::builder()
                    .with_tonic()
                    .with_tls_config(target.grpc_tls()?)
                    .with_timeout(target.timeout);
                if let Some(endpoint) = &target.endpoint {
                    builder = builder.with_endpoint(endpoint.clone());
                }
                if !target.headers.is_empty() {
                    builder = builder.with_metadata(target.grpc_metadata()?);
                }
                builder.build()
            }
            OtlpProtocol::HttpProtobuf => {
                let client = ExportHttpClient::new(
                    runtime.handle().clone(),
                    &target.rustls_config()?,
                    target.timeout,
                    Arc::clone(&target.failures),
                );
                let mut builder = <$exporter>::builder()
                    .with_http()
                    .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
                    .with_http_client(client)
                    .with_timeout(target.timeout);
                if let Some(endpoint) = &target.endpoint {
                    builder = builder.with_endpoint(endpoint.clone());
                }
                if !target.headers.is_empty() {
                    builder = builder.with_headers(target.headers.iter().cloned().collect());
                }
                builder.build()
            }
        }
        .map_err(|e| TelemetryError::Exporter {
            signal: target.signal.name(),
            source: e,
        })
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
