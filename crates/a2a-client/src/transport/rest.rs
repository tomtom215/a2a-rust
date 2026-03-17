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

// ── Type aliases ──────────────────────────────────────────────────────────────

#[cfg(not(feature = "tls-rustls"))]
type HttpClient = Client<HttpConnector, Full<Bytes>>;

#[cfg(feature = "tls-rustls")]
type HttpClient = crate::tls::HttpsClient;

// ── Route ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpMethod {
    Get,
    Post,
    Delete,
}

#[derive(Debug)]
struct Route {
    http_method: HttpMethod,
    path_template: &'static str,
    /// Names of params that are path parameters (extracted from JSON params).
    path_params: &'static [&'static str],
    /// Whether the response is SSE (used in tests).
    #[allow(dead_code)]
    streaming: bool,
}

// ── Method routing ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn route_for(method: &str) -> Option<Route> {
    match method {
        "SendMessage" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/message:send",
            path_params: &[],
            streaming: false,
        }),
        "SendStreamingMessage" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/message:stream",
            path_params: &[],
            streaming: true,
        }),
        "GetTask" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/tasks/{id}",
            path_params: &["id"],
            streaming: false,
        }),
        "CancelTask" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/tasks/{id}:cancel",
            path_params: &["id"],
            streaming: false,
        }),
        "ListTasks" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/tasks",
            path_params: &[],
            streaming: false,
        }),
        "SubscribeToTask" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/tasks/{id}:subscribe",
            path_params: &["id"],
            streaming: true,
        }),
        "CreateTaskPushNotificationConfig" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/tasks/{taskId}/pushNotificationConfigs",
            path_params: &["taskId"],
            streaming: false,
        }),
        "GetTaskPushNotificationConfig" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/tasks/{taskId}/pushNotificationConfigs/{id}",
            path_params: &["taskId", "id"],
            streaming: false,
        }),
        "ListTaskPushNotificationConfigs" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/tasks/{taskId}/pushNotificationConfigs",
            path_params: &["taskId"],
            streaming: false,
        }),
        "DeleteTaskPushNotificationConfig" => Some(Route {
            http_method: HttpMethod::Delete,
            path_template: "/tasks/{taskId}/pushNotificationConfigs/{id}",
            path_params: &["taskId", "id"],
            streaming: false,
        }),
        "GetExtendedAgentCard" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/extendedAgentCard",
            path_params: &[],
            streaming: false,
        }),
        _ => None,
    }
}

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
                ClientError::Transport("request timed out".into())
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

// ── Query string builder ─────────────────────────────────────────────────────

/// Builds a URL query string from a JSON object's non-null fields.
///
/// Values are percent-encoded per RFC 3986 to avoid query-string
/// injection (e.g., values containing `&`, `=`, or spaces).
fn build_query_string(params: &serde_json::Value) -> String {
    let Some(obj) = params.as_object() else {
        return String::new();
    };
    let mut parts = Vec::new();
    for (k, v) in obj {
        let raw = match v {
            serde_json::Value::Null => continue,
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => match serde_json::to_string(v) {
                Ok(s) => s,
                Err(_) => continue,
            },
        };
        parts.push(format!(
            "{}={}",
            encode_query_value(k),
            encode_query_value(&raw)
        ));
    }
    parts.join("&")
}

/// Percent-encodes a string for safe use in a URL query parameter.
///
/// Encodes all characters except unreserved characters (RFC 3986 §2.3):
/// `A-Z a-z 0-9 - . _ ~`
fn encode_query_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX_CHARS[(b >> 4) as usize]));
                out.push(char::from(HEX_CHARS[(b & 0x0F) as usize]));
            }
        }
    }
    out
}

const HEX_CHARS: [u8; 16] = *b"0123456789ABCDEF";

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_for_known_methods() {
        assert!(route_for("SendMessage").is_some());
        assert!(route_for("GetTask").is_some());
        assert!(route_for("ListTasks").is_some());
        assert!(route_for("SendStreamingMessage").is_some_and(|r| r.streaming));
    }

    #[test]
    fn route_for_unknown_method_returns_none() {
        assert!(route_for("unknown/method").is_none());
    }

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

    #[test]
    fn encode_query_value_encodes_special_chars() {
        // Spaces
        assert_eq!(super::encode_query_value("hello world"), "hello%20world");
        // Ampersand & equals — would break query string parsing if unencoded
        assert_eq!(super::encode_query_value("a=1&b=2"), "a%3D1%26b%3D2");
        // Percent sign itself
        assert_eq!(super::encode_query_value("100%"), "100%25");
        // Unreserved characters pass through
        assert_eq!(
            super::encode_query_value("safe-._~AZaz09"),
            "safe-._~AZaz09"
        );
    }

    #[test]
    fn build_query_string_encodes_values() {
        let params = serde_json::json!({
            "filter": "status=active&role=admin",
            "name": "John Doe"
        });
        let qs = super::build_query_string(&params);
        // Values should be percent-encoded
        assert!(qs.contains("filter=status%3Dactive%26role%3Dadmin"));
        assert!(qs.contains("name=John%20Doe"));
    }
}
