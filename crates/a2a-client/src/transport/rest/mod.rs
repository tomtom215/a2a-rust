// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! HTTP REST transport implementation.
//!
//! [`RestTransport`] maps A2A method names to REST HTTP verb + path
//! combinations, extracts path parameters from the JSON params, and sends
//! standard JSON bodies.
//!
//! # Method → REST mapping
//!
//! | A2A method | HTTP verb | Path |
//! |---|---|---|
//! | `SendMessage` | POST | `/message:send` |
//! | `SendStreamingMessage` | POST | `/message:stream` |
//! | `GetTask` | GET | `/tasks/{id}` |
//! | `CancelTask` | POST | `/tasks/{id}:cancel` |
//! | `ListTasks` | GET | `/tasks` |
//! | `SubscribeToTask` | POST | `/tasks/{id}:subscribe` |
//! | `CreateTaskPushNotificationConfig` | POST | `/tasks/{id}/pushNotificationConfigs` |
//! | `GetTaskPushNotificationConfig` | GET | `/tasks/{id}/pushNotificationConfigs/{configId}` |
//! | `ListTaskPushNotificationConfigs` | GET | `/tasks/{id}/pushNotificationConfigs` |
//! | `DeleteTaskPushNotificationConfig` | DELETE | `/tasks/{id}/pushNotificationConfigs/{configId}` |
//! | `GetExtendedAgentCard` | GET | `/extendedAgentCard` |

mod query;
mod routing;

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

use a2a_protocol_types::JsonRpcResponse;

use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

use query::build_query_string;
use routing::{route_for, HttpMethod, Route};

// ── Type aliases ──────────────────────────────────────────────────────────────

#[cfg(not(feature = "tls-rustls"))]
type HttpClient = Client<HttpConnector, Full<Bytes>>;

#[cfg(feature = "tls-rustls")]
type HttpClient = crate::tls::HttpsClient;

// ── RestTransport ─────────────────────────────────────────────────────────────

/// REST transport: HTTP verbs mapped to REST paths.
///
/// Create via [`RestTransport::new`] or let [`crate::ClientBuilder`] construct
/// one from the agent card.
#[derive(Clone, Debug)]
pub struct RestTransport {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    client: HttpClient,
    base_url: String,
    request_timeout: Duration,
    stream_connect_timeout: Duration,
}

impl RestTransport {
    /// Creates a new transport using `base_url` as the root URL.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidEndpoint`] if the URL is malformed.
    pub fn new(base_url: impl Into<String>) -> ClientResult<Self> {
        Self::with_timeout(base_url, Duration::from_secs(30))
    }

    /// Creates a new transport with a custom request timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidEndpoint`] if the URL is malformed.
    pub fn with_timeout(
        base_url: impl Into<String>,
        request_timeout: Duration,
    ) -> ClientResult<Self> {
        Self::with_timeouts(base_url, request_timeout, request_timeout)
    }

    /// Creates a new transport with separate request and stream connect timeouts.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidEndpoint`] if the URL is malformed.
    pub fn with_timeouts(
        base_url: impl Into<String>,
        request_timeout: Duration,
        stream_connect_timeout: Duration,
    ) -> ClientResult<Self> {
        let base_url = base_url.into();
        if base_url.is_empty()
            || (!base_url.starts_with("http://") && !base_url.starts_with("https://"))
        {
            return Err(ClientError::InvalidEndpoint(format!(
                "invalid base URL: {base_url}"
            )));
        }

        #[cfg(not(feature = "tls-rustls"))]
        let client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();

        #[cfg(feature = "tls-rustls")]
        let client = crate::tls::build_https_client();

        Ok(Self {
            inner: Arc::new(Inner {
                client,
                base_url: base_url.trim_end_matches('/').to_owned(),
                request_timeout,
                stream_connect_timeout,
            }),
        })
    }

    /// Returns the base URL this transport targets.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    // ── internals ─────────────────────────────────────────────────────────────

    fn build_uri(
        &self,
        route: &Route,
        params: &serde_json::Value,
    ) -> ClientResult<(String, serde_json::Value)> {
        let mut path = route.path_template.to_owned();
        let mut remaining = params.clone();

        for &param in route.path_params {
            let value = remaining
                .get(param)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| ClientError::Transport(format!("missing path parameter: {param}")))?
                .to_owned();

            path = path.replace(&format!("{{{param}}}"), &value);

            if let Some(obj) = remaining.as_object_mut() {
                obj.remove(param);
            }
        }

        let mut uri = format!("{}{path}", self.inner.base_url);

        // For GET/DELETE, append remaining params as query string.
        if route.http_method == HttpMethod::Get || route.http_method == HttpMethod::Delete {
            let query = build_query_string(&remaining);
            if !query.is_empty() {
                uri.push('?');
                uri.push_str(&query);
            }
        }

        Ok((uri, remaining))
    }

    fn build_request(
        &self,
        method: &str,
        params: &serde_json::Value,
        extra_headers: &HashMap<String, String>,
        streaming: bool,
    ) -> ClientResult<hyper::Request<Full<Bytes>>> {
        let route = route_for(method)
            .ok_or_else(|| ClientError::Transport(format!("no REST route for method: {method}")))?;

        let (uri, body_params) = self.build_uri(&route, params)?;
        let accept = if streaming {
            "text/event-stream"
        } else {
            "application/json"
        };

        let hyper_method = match route.http_method {
            HttpMethod::Get => hyper::Method::GET,
            HttpMethod::Post => hyper::Method::POST,
            HttpMethod::Delete => hyper::Method::DELETE,
        };

        let body =
            if route.http_method == HttpMethod::Get || route.http_method == HttpMethod::Delete {
                // For GET/DELETE, body is empty; params were in the path.
                Full::new(Bytes::new())
            } else {
                let bytes = serde_json::to_vec(&body_params).map_err(ClientError::Serialization)?;
                Full::new(Bytes::from(bytes))
            };

        let mut builder = hyper::Request::builder()
            .method(hyper_method)
            .uri(uri)
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
            .body(body)
            .map_err(|e| ClientError::Transport(e.to_string()))
    }

    async fn execute_request(
        &self,
        method: &str,
        params: serde_json::Value,
        extra_headers: &HashMap<String, String>,
    ) -> ClientResult<serde_json::Value> {
        trace_info!(method, base_url = %self.inner.base_url, "sending REST request");

        let req = self.build_request(method, &params, extra_headers, false)?;

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
            return Err(ClientError::UnexpectedStatus {
                status: status.as_u16(),
                body: super::truncate_body(&body_str),
            });
        }

        // REST responses may or may not wrap in JSON-RPC; try JSON-RPC first.
        if let Ok(envelope) =
            serde_json::from_slice::<JsonRpcResponse<serde_json::Value>>(&body_bytes)
        {
            return match envelope {
                JsonRpcResponse::Success(ok) => Ok(ok.result),
                JsonRpcResponse::Error(err) => {
                    let a2a = a2a_protocol_types::A2aError::new(
                        a2a_protocol_types::ErrorCode::try_from(err.error.code)
                            .unwrap_or(a2a_protocol_types::ErrorCode::InternalError),
                        err.error.message,
                    );
                    Err(ClientError::Protocol(a2a))
                }
            };
        }

        // Fall back to raw JSON value.
        serde_json::from_slice(&body_bytes).map_err(ClientError::Serialization)
    }

    async fn execute_streaming_request(
        &self,
        method: &str,
        params: serde_json::Value,
        extra_headers: &HashMap<String, String>,
    ) -> ClientResult<EventStream> {
        trace_info!(method, base_url = %self.inner.base_url, "opening REST SSE stream");

        let req = self.build_request(method, &params, extra_headers, true)?;

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

        let task_handle = tokio::spawn(async move {
            body_reader_task(body, tx).await;
        });

        Ok(EventStream::with_abort_handle(
            rx,
            task_handle.abort_handle(),
        ))
    }
}

impl Transport for RestTransport {
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

// ── Body reader task (shared with jsonrpc.rs pattern) ─────────────────────────

async fn body_reader_task(
    body: hyper::body::Incoming,
    tx: mpsc::Sender<crate::streaming::event_stream::BodyChunk>,
) {
    tokio::pin!(body);
    loop {
        let frame = std::future::poll_fn(|cx| {
            use hyper::body::Body;
            // SAFETY: `body` is pinned by `tokio::pin!` and not moved.
            let pinned = unsafe { Pin::new_unchecked(&mut *body) };
            pinned.poll_frame(cx)
        })
        .await;

        match frame {
            None => break,
            Some(Err(e)) => {
                let _ = tx.send(Err(ClientError::Http(e))).await;
                break;
            }
            Some(Ok(f)) => {
                if let Ok(data) = f.into_data() {
                    if tx.send(Ok(data)).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_uri_extracts_path_param_and_appends_query() {
        let transport = RestTransport::new("http://localhost:8080").unwrap();
        let route = route_for("GetTask").unwrap();
        let params = serde_json::json!({"id": "task-123", "historyLength": 5});
        let (uri, _remaining) = transport.build_uri(&route, &params).unwrap();
        assert!(
            uri.starts_with("http://localhost:8080/tasks/task-123"),
            "should have task ID in path"
        );
        assert!(
            uri.contains("historyLength=5"),
            "should have historyLength in query"
        );
    }

    #[test]
    fn build_uri_appends_query_for_get() {
        let transport = RestTransport::new("http://localhost:8080").unwrap();
        let route = route_for("ListTasks").unwrap();
        let params = serde_json::json!({"pageSize": 10});
        let (uri, _remaining) = transport.build_uri(&route, &params).unwrap();
        assert!(uri.contains("pageSize=10"), "should have pageSize in query");
    }

    #[test]
    fn rest_transport_rejects_invalid_url() {
        assert!(RestTransport::new("not-a-url").is_err());
    }

    #[test]
    fn rest_transport_stores_base_url() {
        let t = RestTransport::new("http://localhost:9090").unwrap();
        assert_eq!(t.base_url(), "http://localhost:9090");
    }

    // ── Mutation-killing tests for build_uri / build_request query param logic ──

    #[test]
    fn build_uri_post_does_not_append_query_params() {
        let transport = RestTransport::new("http://localhost:8080").unwrap();
        let route = route_for("SendMessage").unwrap();
        assert_eq!(route.http_method, HttpMethod::Post);
        let params = serde_json::json!({"key": "value", "extra": 42});
        let (uri, _remaining) = transport.build_uri(&route, &params).unwrap();
        assert!(
            !uri.contains('?'),
            "POST request should not have query params in URI, got: {uri}"
        );
    }

    #[test]
    fn build_uri_delete_appends_query_params() {
        let transport = RestTransport::new("http://localhost:8080").unwrap();
        let route = route_for("DeleteTaskPushNotificationConfig").unwrap();
        assert_eq!(route.http_method, HttpMethod::Delete);
        let params = serde_json::json!({"taskId": "t1", "id": "c1", "extra": "val"});
        let (uri, _remaining) = transport.build_uri(&route, &params).unwrap();
        assert!(
            uri.contains("extra=val"),
            "DELETE request should have remaining params in query string, got: {uri}"
        );
    }

    #[test]
    fn build_request_post_has_json_body() {
        use hyper::body::Body;
        let transport = RestTransport::new("http://localhost:8080").unwrap();
        let params = serde_json::json!({"message": {"role": "user", "parts": []}});
        let req = transport
            .build_request("SendMessage", &params, &HashMap::new(), false)
            .unwrap();
        assert_eq!(req.method(), hyper::Method::POST);
        // POST should have a non-empty body
        let size = req.body().size_hint().exact().unwrap_or(0);
        assert!(size > 0, "POST body should not be empty");
    }

    #[test]
    fn build_request_get_has_empty_body() {
        use hyper::body::Body;
        let transport = RestTransport::new("http://localhost:8080").unwrap();
        let params = serde_json::json!({"id": "task-1"});
        let req = transport
            .build_request("GetTask", &params, &HashMap::new(), false)
            .unwrap();
        assert_eq!(req.method(), hyper::Method::GET);
        let size = req.body().size_hint().exact().unwrap_or(1);
        assert_eq!(size, 0, "GET body should be empty");
    }

    #[test]
    fn build_request_delete_has_empty_body() {
        use hyper::body::Body;
        let transport = RestTransport::new("http://localhost:8080").unwrap();
        let params = serde_json::json!({"taskId": "t1", "id": "c1"});
        let req = transport
            .build_request(
                "DeleteTaskPushNotificationConfig",
                &params,
                &HashMap::new(),
                false,
            )
            .unwrap();
        assert_eq!(req.method(), hyper::Method::DELETE);
        let size = req.body().size_hint().exact().unwrap_or(1);
        assert_eq!(size, 0, "DELETE body should be empty");
    }

    // ── HTTP server tests for execute_request / execute_streaming_request ──

    async fn start_rest_server(
        status: u16,
        content_type: &'static str,
        body: impl Into<String>,
    ) -> std::net::SocketAddr {
        let body: String = body.into();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let io = hyper_util::rt::TokioIo::new(stream);
                let body = body.clone();
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |_req| {
                        let body = body.clone();
                        async move {
                            Ok::<_, hyper::Error>(
                                hyper::Response::builder()
                                    .status(status)
                                    .header("content-type", content_type)
                                    .body(Full::new(Bytes::from(body)))
                                    .unwrap(),
                            )
                        }
                    });
                    let _ = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(io, service)
                    .await;
                });
            }
        });

        addr
    }

    #[tokio::test]
    async fn execute_request_non_success_returns_error() {
        let addr = start_rest_server(500, "text/plain", "Internal Server Error").await;
        let url = format!("http://127.0.0.1:{}", addr.port());
        let transport = RestTransport::new(&url).unwrap();
        let result = transport
            .execute_request("GetTask", serde_json::json!({"id": "t1"}), &HashMap::new())
            .await;
        match result {
            Err(ClientError::UnexpectedStatus { status, .. }) => {
                assert_eq!(status, 500);
            }
            other => panic!("expected UnexpectedStatus, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_request_success_parses_json() {
        let response_body = r#"{"jsonrpc":"2.0","id":"1","result":{"hello":"world"}}"#;
        let addr = start_rest_server(200, "application/json", response_body).await;
        let url = format!("http://127.0.0.1:{}", addr.port());
        let transport = RestTransport::new(&url).unwrap();
        let result = transport
            .execute_request("GetTask", serde_json::json!({"id": "t1"}), &HashMap::new())
            .await;
        let value = result.unwrap();
        assert_eq!(value["hello"], "world");
    }

    #[tokio::test]
    async fn execute_streaming_request_non_success_returns_error() {
        let addr = start_rest_server(500, "text/plain", "Internal Server Error").await;
        let url = format!("http://127.0.0.1:{}", addr.port());
        let transport = RestTransport::new(&url).unwrap();
        let result = transport
            .execute_streaming_request(
                "SendStreamingMessage",
                serde_json::json!({}),
                &HashMap::new(),
            )
            .await;
        match result {
            Err(ClientError::UnexpectedStatus { status, .. }) => {
                assert_eq!(status, 500);
            }
            other => panic!("expected UnexpectedStatus, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_streaming_request_success_returns_event_stream() {
        // Start a server that returns SSE data with a valid JSON-RPC wrapped StreamResponse.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let io = hyper_util::rt::TokioIo::new(stream);
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(|_req| async {
                        let sse_body = "data: {\"jsonrpc\":\"2.0\",\"id\":\"1\",\"result\":{\"status\":\"ok\"}}\n\n";
                        Ok::<_, hyper::Error>(
                            hyper::Response::builder()
                                .status(200)
                                .header("content-type", "text/event-stream")
                                .body(Full::new(Bytes::from(sse_body)))
                                .unwrap(),
                        )
                    });
                    let _ = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(io, service)
                    .await;
                });
            }
        });

        let url = format!("http://127.0.0.1:{}", addr.port());
        let transport = RestTransport::new(&url).unwrap();
        let mut stream = transport
            .execute_streaming_request(
                "SendStreamingMessage",
                serde_json::json!({}),
                &HashMap::new(),
            )
            .await
            .unwrap();
        // The EventStream should yield at least one event from body_reader_task.
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
            .await
            .expect("timed out waiting for event");
        assert!(
            event.is_some(),
            "expected at least one event from the stream"
        );
    }
}
