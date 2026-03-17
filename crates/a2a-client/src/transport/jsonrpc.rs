// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! JSON-RPC 2.0 over HTTP transport implementation.
//!
//! [`JsonRpcTransport`] sends every A2A method call as an HTTP POST to the
//! agent's single JSON-RPC endpoint URL. Streaming requests include
//! `Accept: text/event-stream` and the response body is consumed as SSE.
//!
//! # Connection pooling
//!
//! The underlying [`hyper_util::client::legacy::Client`] pools connections
//! across requests. Cloning [`JsonRpcTransport`] is cheap — it clones the
//! inner `Arc`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header;
#[cfg(not(feature = "tls-rustls"))]
use hyper_util::client::legacy::connect::HttpConnector;
#[cfg(not(feature = "tls-rustls"))]
use hyper_util::client::legacy::Client;
#[cfg(not(feature = "tls-rustls"))]
use hyper_util::rt::TokioExecutor;
use tokio::sync::mpsc;
use uuid::Uuid;

use a2a_protocol_types::{JsonRpcRequest, JsonRpcResponse};

use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

// ── Type aliases ──────────────────────────────────────────────────────────────

#[cfg(not(feature = "tls-rustls"))]
type HttpClient = Client<HttpConnector, Full<Bytes>>;

#[cfg(feature = "tls-rustls")]
type HttpClient = crate::tls::HttpsClient;

// ── JsonRpcTransport ──────────────────────────────────────────────────────────

/// JSON-RPC 2.0 transport: HTTP POST to a single endpoint.
///
/// Create via [`JsonRpcTransport::new`] or let [`crate::ClientBuilder`]
/// construct one automatically from the agent card.
#[derive(Clone, Debug)]
pub struct JsonRpcTransport {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    client: HttpClient,
    endpoint: String,
    request_timeout: Duration,
    stream_connect_timeout: Duration,
}

impl JsonRpcTransport {
    /// Creates a new transport targeting the given endpoint URL.
    ///
    /// The endpoint is typically the `url` field from an [`a2a_protocol_types::AgentCard`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidEndpoint`] if the URL is malformed.
    pub fn new(endpoint: impl Into<String>) -> ClientResult<Self> {
        Self::with_timeout(endpoint, Duration::from_secs(30))
    }

    /// Creates a new transport with a custom request timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidEndpoint`] if the URL is malformed.
    pub fn with_timeout(
        endpoint: impl Into<String>,
        request_timeout: Duration,
    ) -> ClientResult<Self> {
        Self::with_timeouts(endpoint, request_timeout, request_timeout)
    }

    /// Creates a new transport with separate request and stream connect timeouts.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidEndpoint`] if the URL is malformed.
    pub fn with_timeouts(
        endpoint: impl Into<String>,
        request_timeout: Duration,
        stream_connect_timeout: Duration,
    ) -> ClientResult<Self> {
        let endpoint = endpoint.into();
        validate_url(&endpoint)?;

        #[cfg(not(feature = "tls-rustls"))]
        let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();

        #[cfg(feature = "tls-rustls")]
        let client = crate::tls::build_https_client();

        Ok(Self {
            inner: Arc::new(Inner {
                client,
                endpoint,
                request_timeout,
                stream_connect_timeout,
            }),
        })
    }

    /// Returns the endpoint URL this transport targets.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.inner.endpoint
    }

    // ── internals ─────────────────────────────────────────────────────────────

    fn build_request(
        &self,
        method: &str,
        params: serde_json::Value,
        extra_headers: &HashMap<String, String>,
        accept_sse: bool,
    ) -> ClientResult<hyper::Request<Full<Bytes>>> {
        let id = serde_json::Value::String(Uuid::new_v4().to_string());
        let rpc_req = JsonRpcRequest::with_params(id, method, params);
        let body_bytes = serde_json::to_vec(&rpc_req).map_err(ClientError::Serialization)?;

        let accept = if accept_sse {
            "text/event-stream"
        } else {
            "application/json"
        };

        let mut builder = hyper::Request::builder()
            .method(hyper::Method::POST)
            .uri(&self.inner.endpoint)
            .header(header::CONTENT_TYPE, a2a_protocol_types::A2A_CONTENT_TYPE)
            .header(
                a2a_protocol_types::A2A_VERSION_HEADER,
                a2a_protocol_types::A2A_VERSION,
            )
            .header(header::ACCEPT, accept);

        for (k, v) in extra_headers {
            builder = builder.header(k.as_str(), v.as_str());
        }

        builder
            .body(Full::new(Bytes::from(body_bytes)))
            .map_err(|e| ClientError::Transport(e.to_string()))
    }

    async fn execute_request(
        &self,
        method: &str,
        params: serde_json::Value,
        extra_headers: &HashMap<String, String>,
    ) -> ClientResult<serde_json::Value> {
        trace_info!(method, endpoint = %self.inner.endpoint, "sending JSON-RPC request");

        let req = self.build_request(method, params, extra_headers, false)?;

        let resp = tokio::time::timeout(self.inner.request_timeout, self.inner.client.request(req))
            .await
            .map_err(|_| {
                trace_error!(method, "request timed out");
                ClientError::Timeout("request timed out".into())
            })?
            .map_err(|e| {
                trace_error!(method, error = %e, "HTTP client error");
                ClientError::HttpClient(e.to_string())
            })?;

        let status = resp.status();
        trace_debug!(method, %status, "received response");

        let body_bytes = resp.collect().await.map_err(ClientError::Http)?.to_bytes();

        if !status.is_success() {
            let body_str = String::from_utf8_lossy(&body_bytes);
            trace_warn!(method, %status, "unexpected HTTP status");
            return Err(ClientError::UnexpectedStatus {
                status: status.as_u16(),
                body: super::truncate_body(&body_str),
            });
        }

        let envelope: JsonRpcResponse<serde_json::Value> = serde_json::from_slice(&body_bytes)
            .map_err(|e| {
                // If the response isn't valid JSON-RPC, the server may use a
                // different protocol binding (e.g. REST).
                let preview = String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(200)]);
                if preview.contains("jsonrpc") {
                    ClientError::Serialization(e)
                } else {
                    ClientError::ProtocolBindingMismatch(format!(
                        "response is not JSON-RPC ({e}); the server may use REST transport",
                    ))
                }
            })?;

        match envelope {
            JsonRpcResponse::Success(ok) => {
                trace_info!(method, "request succeeded");
                Ok(ok.result)
            }
            JsonRpcResponse::Error(err) => {
                trace_warn!(method, code = err.error.code, "JSON-RPC error response");
                let a2a = a2a_protocol_types::A2aError::new(
                    a2a_protocol_types::ErrorCode::try_from(err.error.code)
                        .unwrap_or(a2a_protocol_types::ErrorCode::InternalError),
                    err.error.message,
                );
                Err(ClientError::Protocol(a2a))
            }
        }
    }

    async fn execute_streaming_request(
        &self,
        method: &str,
        params: serde_json::Value,
        extra_headers: &HashMap<String, String>,
    ) -> ClientResult<EventStream> {
        trace_info!(method, endpoint = %self.inner.endpoint, "opening SSE stream");

        let req = self.build_request(method, params, extra_headers, true)?;

        let resp = tokio::time::timeout(
            self.inner.stream_connect_timeout,
            self.inner.client.request(req),
        )
        .await
        .map_err(|_| {
            trace_error!(method, "stream connect timed out");
            ClientError::Timeout("stream connect timed out".into())
        })?
        .map_err(|e| {
            trace_error!(method, error = %e, "HTTP client error");
            ClientError::HttpClient(e.to_string())
        })?;

        let status = resp.status();
        if !status.is_success() {
            let body_bytes = resp.collect().await.map_err(ClientError::Http)?.to_bytes();
            let body_str = String::from_utf8_lossy(&body_bytes);
            return Err(ClientError::UnexpectedStatus {
                status: status.as_u16(),
                body: super::truncate_body(&body_str),
            });
        }

        let (tx, rx) = mpsc::channel::<crate::streaming::event_stream::BodyChunk>(64);
        let body = resp.into_body();

        // Spawn a background task that reads body chunks and forwards them.
        let task_handle = tokio::spawn(async move {
            body_reader_task(body, tx).await;
        });

        Ok(EventStream::with_abort_handle(
            rx,
            task_handle.abort_handle(),
        ))
    }
}

impl Transport for JsonRpcTransport {
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        Box::pin(self.execute_request(method, params, extra_headers))
    }

    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(self.execute_streaming_request(method, params, extra_headers))
    }
}

// ── Body reader task ──────────────────────────────────────────────────────────

/// Background task: reads chunks from a hyper response body and forwards them
/// to the SSE channel.
///
/// Exits when the body is exhausted or the channel receiver is dropped.
async fn body_reader_task(
    body: hyper::body::Incoming,
    tx: mpsc::Sender<crate::streaming::event_stream::BodyChunk>,
) {
    // We need to pin the Incoming body before polling it.
    // Safety: we do not move `body` after this point.
    tokio::pin!(body);

    loop {
        // Poll one frame from the body.
        let frame = std::future::poll_fn(|cx| {
            use hyper::body::Body;
            // SAFETY: `body` is pinned by `tokio::pin!` above and we do not
            // move it. `Pin::new_unchecked` is safe here because the future
            // created by `poll_fn` ensures stable addressing.
            let pinned = unsafe { Pin::new_unchecked(&mut *body) };
            pinned.poll_frame(cx)
        })
        .await;

        match frame {
            None => break, // body exhausted
            Some(Err(e)) => {
                let _ = tx.send(Err(ClientError::Http(e))).await;
                break;
            }
            Some(Ok(frame)) => {
                if let Ok(data) = frame.into_data() {
                    if tx.send(Ok(data)).await.is_err() {
                        // Receiver dropped; stop reading.
                        break;
                    }
                }
                // Non-data frames (trailers) are skipped.
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn validate_url(url: &str) -> ClientResult<()> {
    if url.is_empty() {
        return Err(ClientError::InvalidEndpoint("URL must not be empty".into()));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ClientError::InvalidEndpoint(format!(
            "URL must start with http:// or https://: {url}"
        )));
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_url_rejects_empty() {
        assert!(validate_url("").is_err());
    }

    #[test]
    fn validate_url_rejects_non_http() {
        assert!(validate_url("ftp://example.com").is_err());
    }

    #[test]
    fn validate_url_accepts_http() {
        assert!(validate_url("http://localhost:8080").is_ok());
    }

    #[test]
    fn validate_url_accepts_https() {
        assert!(validate_url("https://agent.example.com/a2a").is_ok());
    }

    #[test]
    fn transport_new_rejects_bad_url() {
        assert!(JsonRpcTransport::new("not-a-url").is_err());
    }

    #[test]
    fn transport_new_stores_endpoint() {
        let t = JsonRpcTransport::new("http://localhost:9090").unwrap();
        assert_eq!(t.endpoint(), "http://localhost:9090");
    }
}
