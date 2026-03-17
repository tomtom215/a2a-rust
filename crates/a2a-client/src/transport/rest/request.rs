// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Request building and execution for the REST transport.
//!
//! Handles URI construction (path parameter interpolation + query strings),
//! HTTP request assembly, and synchronous (non-streaming) request execution.

use std::collections::HashMap;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header;

use a2a_protocol_types::JsonRpcResponse;

use crate::error::{ClientError, ClientResult};

use super::query::build_query_string;
use super::routing::{route_for, HttpMethod, Route};
use super::RestTransport;

impl RestTransport {
    pub(super) fn build_uri(
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

    pub(super) fn build_request(
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

    pub(super) async fn execute_request(
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
                body: super::super::truncate_body(&body_str),
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
}

#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use hyper::body::Bytes;

    use super::super::routing::{route_for, HttpMethod};
    use super::super::*;

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

    // ── HTTP server tests for execute_request ──

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
}
