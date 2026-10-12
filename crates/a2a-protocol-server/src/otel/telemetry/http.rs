// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The HTTP client OTLP/HTTP export goes through: hyper and rustls (ring),
//! the stack the rest of this crate uses, on the export runtime.
//!
//! `opentelemetry-otlp` offers reqwest, which `deny.toml` keeps out of this
//! crate, or `opentelemetry-http`'s hyper client, which uses Tokio's timer
//! and executor wherever it is polled. The SDK polls it on its batch
//! processor's own thread, which has no runtime, so that client panics there
//! with "there is no reactor running". This one moves every request onto
//! [`ExportRuntime`](super::runtime::ExportRuntime) and waits for it.

use std::time::Duration;

use bytes::Bytes;
use http::{Request, Response};
use http_body_util::{BodyExt as _, Full, Limited};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use opentelemetry_http::{HttpClient, HttpError};
use tokio::runtime::Handle;

/// The most of a collector's response body read. An OTLP response is a
/// short protobuf or JSON status; the body is read only so the connection can
/// be reused, and a misbehaving endpoint does not get to choose how much
/// memory that takes.
pub(super) const MAX_RESPONSE_BODY: usize = 64 * 1024;

type Https = hyper_rustls::HttpsConnector<HttpConnector>;

/// An [`HttpClient`] that runs each request on the export runtime.
#[derive(Clone)]
pub(super) struct ExportHttpClient {
    handle: Handle,
    client: Client<Https, Full<Bytes>>,
    timeout: Duration,
}

impl std::fmt::Debug for ExportHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportHttpClient")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl ExportHttpClient {
    /// A client verifying servers against `tls`, giving up on a request
    /// after `timeout`.
    pub(super) fn new(handle: Handle, tls: &rustls::ClientConfig, timeout: Duration) -> Self {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls.clone())
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        let client = Client::builder(TokioExecutor::new()).build(connector);
        Self {
            handle,
            client,
            timeout,
        }
    }
}

#[async_trait::async_trait]
impl HttpClient for ExportHttpClient {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let client = self.client.clone();
        let timeout = self.timeout;
        let exchange = async move {
            // One deadline for the whole exchange, response and body: the
            // timeout is the export's (`OTEL_EXPORTER_OTLP_TIMEOUT`), not each
            // phase's.
            let deadline = tokio::time::Instant::now() + timeout;
            let (parts, body) = request.into_parts();
            let response = client.request(Request::from_parts(parts, Full::new(body)));
            let response = tokio::time::timeout_at(deadline, response)
                .await
                .map_err(|_| format!("no response within {timeout:?}"))??;
            let (parts, body) = response.into_parts();
            let body =
                tokio::time::timeout_at(deadline, Limited::new(body, MAX_RESPONSE_BODY).collect())
                    .await
                    .map_err(|_| format!("response not read within {timeout:?}"))??
                    .to_bytes();
            Ok::<_, HttpError>(Response::from_parts(parts, body))
        };
        // A `JoinError` means the export runtime stopped first — `Telemetry`
        // stops it only after the providers' final flush, so this is the
        // flush of a provider someone kept alive past the guard.
        match self.handle.spawn(exchange).await {
            Ok(result) => result,
            Err(e) => Err(e.into()),
        }
    }
}
