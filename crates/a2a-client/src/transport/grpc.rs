// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! gRPC transport implementation for the A2A client.
//!
//! [`GrpcTransport`] connects to a tonic-served A2A gRPC endpoint and
//! implements the [`Transport`] trait. JSON payloads are carried inside
//! protobuf `bytes` fields, reusing the same serde types as JSON-RPC
//! and REST.
//!
//! # Configuration
//!
//! Use [`GrpcTransportConfig`] to control timeouts and message sizes.
//!
//! # Example
//!
//! ```rust,no_run
//! # async fn example() -> Result<(), a2a_protocol_client::error::ClientError> {
//! use a2a_protocol_client::transport::grpc::GrpcTransport;
//! use a2a_protocol_client::ClientBuilder;
//!
//! let transport = GrpcTransport::connect("http://localhost:50051").await?;
//! let client = ClientBuilder::new("http://localhost:50051")
//!     .with_custom_transport(transport)
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tonic::transport::Channel;

use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

// Include the generated protobuf client code.
mod proto {
    #![allow(
        clippy::all,
        clippy::pedantic,
        clippy::nursery,
        missing_docs,
        unused_qualifications
    )]
    tonic::include_proto!("a2a.v1");
}

use proto::a2a_service_client::A2aServiceClient;
use proto::JsonPayload;

// ── GrpcTransportConfig ─────────────────────────────────────────────────────

/// Configuration for the gRPC transport.
///
/// # Example
///
/// ```rust
/// use a2a_protocol_client::transport::grpc::GrpcTransportConfig;
/// use std::time::Duration;
///
/// let config = GrpcTransportConfig::default()
///     .with_timeout(Duration::from_secs(60))
///     .with_max_message_size(8 * 1024 * 1024);
/// ```
#[derive(Debug, Clone)]
pub struct GrpcTransportConfig {
    /// Request timeout for unary calls. Default: 30 seconds.
    pub timeout: Duration,
    /// Connection timeout. Default: 10 seconds.
    pub connect_timeout: Duration,
    /// Maximum inbound message size. Default: 4 MiB.
    pub max_message_size: usize,
    /// Channel capacity for streaming responses. Default: 64.
    pub stream_channel_capacity: usize,
}

impl Default for GrpcTransportConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            max_message_size: 4 * 1024 * 1024,
            stream_channel_capacity: 64,
        }
    }
}

impl GrpcTransportConfig {
    /// Sets the unary request timeout.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the connection timeout.
    #[must_use]
    pub const fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the maximum inbound message size.
    #[must_use]
    pub const fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }

    /// Sets the channel capacity for streaming responses.
    #[must_use]
    pub const fn with_stream_channel_capacity(mut self, capacity: usize) -> Self {
        self.stream_channel_capacity = capacity;
        self
    }
}

// ── GrpcTransport ───────────────────────────────────────────────────────────

/// gRPC transport for A2A clients.
///
/// Connects to a tonic-served gRPC endpoint and translates A2A method
/// calls into gRPC RPCs with JSON payloads. Implements the [`Transport`]
/// trait for use with [`crate::A2aClient`].
#[derive(Clone, Debug)]
pub struct GrpcTransport {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    client: tokio::sync::Mutex<A2aServiceClient<Channel>>,
    endpoint: String,
    config: GrpcTransportConfig,
}

impl GrpcTransport {
    /// Connects to a gRPC endpoint with default configuration.
    ///
    /// The endpoint should be an `http://` or `https://` URL.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] if the connection fails.
    pub async fn connect(endpoint: impl Into<String>) -> ClientResult<Self> {
        Self::connect_with_config(endpoint, GrpcTransportConfig::default()).await
    }

    /// Connects to a gRPC endpoint with custom configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] if the connection fails.
    pub async fn connect_with_config(
        endpoint: impl Into<String>,
        config: GrpcTransportConfig,
    ) -> ClientResult<Self> {
        let endpoint_str = endpoint.into();
        validate_url(&endpoint_str)?;

        let channel = tonic::transport::Channel::from_shared(endpoint_str.clone())
            .map_err(|e| ClientError::InvalidEndpoint(format!("invalid gRPC endpoint: {e}")))?
            .connect_timeout(config.connect_timeout)
            .timeout(config.timeout)
            .connect()
            .await
            .map_err(|e| ClientError::Transport(format!("gRPC connect failed: {e}")))?;

        let client = A2aServiceClient::new(channel)
            .max_decoding_message_size(config.max_message_size)
            .max_encoding_message_size(config.max_message_size);

        Ok(Self {
            inner: Arc::new(Inner {
                client: tokio::sync::Mutex::new(client),
                endpoint: endpoint_str,
                config,
            }),
        })
    }

    /// Returns the endpoint URL this transport targets.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.inner.endpoint
    }

    // ── internals ────────────────────────────────────────────────────────

    fn encode_params(params: &serde_json::Value) -> ClientResult<JsonPayload> {
        let data = serde_json::to_vec(params).map_err(ClientError::Serialization)?;
        Ok(JsonPayload { data })
    }

    fn add_metadata(
        req: &mut tonic::Request<JsonPayload>,
        extra_headers: &HashMap<String, String>,
    ) {
        let md = req.metadata_mut();
        md.insert(
            "a2a-version",
            a2a_protocol_types::A2A_VERSION
                .parse()
                .unwrap_or_else(|_| tonic::metadata::MetadataValue::from_static("")),
        );
        for (k, v) in extra_headers {
            if let (Ok(key), Ok(val)) = (
                k.parse::<tonic::metadata::MetadataKey<_>>(),
                v.parse::<tonic::metadata::MetadataValue<_>>(),
            ) {
                md.insert(key, val);
            }
        }
    }

    fn decode_response(payload: &JsonPayload) -> ClientResult<serde_json::Value> {
        serde_json::from_slice(&payload.data).map_err(ClientError::Serialization)
    }

    fn status_to_error(status: &tonic::Status) -> ClientError {
        // FIX(#2): Map deadline/cancellation codes to ClientError::Timeout so
        // they are retryable, matching REST/JSON-RPC timeout behavior.
        match status.code() {
            tonic::Code::DeadlineExceeded => {
                ClientError::Timeout(format!("gRPC deadline exceeded: {}", status.message()))
            }
            tonic::Code::Cancelled => {
                ClientError::Timeout(format!("gRPC request cancelled: {}", status.message()))
            }
            tonic::Code::Unavailable => {
                ClientError::HttpClient(format!("gRPC unavailable: {}", status.message()))
            }
            _ => {
                let a2a = a2a_protocol_types::A2aError::new(
                    grpc_code_to_error_code(status.code()),
                    status.message().to_owned(),
                );
                ClientError::Protocol(a2a)
            }
        }
    }

    #[allow(clippy::significant_drop_tightening)]
    async fn execute_unary(
        &self,
        method: &str,
        params: serde_json::Value,
        extra_headers: &HashMap<String, String>,
    ) -> ClientResult<serde_json::Value> {
        trace_info!(
            method,
            endpoint = %self.inner.endpoint,
            "sending gRPC request"
        );

        let payload = Self::encode_params(&params)?;
        let mut req = tonic::Request::new(payload);
        req.set_timeout(self.inner.config.timeout);
        Self::add_metadata(&mut req, extra_headers);

        // FIX(#11): Wrap the entire gRPC call in tokio::time::timeout for
        // per-request timeout enforcement, matching REST/JSON-RPC behavior.
        let response = tokio::time::timeout(self.inner.config.timeout, async {
            let mut client = self.inner.client.lock().await;
            match method {
                "SendMessage" => client.send_message(req).await,
                "GetTask" => client.get_task(req).await,
                "ListTasks" => client.list_tasks(req).await,
                "CancelTask" => client.cancel_task(req).await,
                "CreateTaskPushNotificationConfig" => {
                    client.create_task_push_notification_config(req).await
                }
                "GetTaskPushNotificationConfig" => {
                    client.get_task_push_notification_config(req).await
                }
                "ListTaskPushNotificationConfigs" => {
                    client.list_task_push_notification_configs(req).await
                }
                "DeleteTaskPushNotificationConfig" => {
                    client.delete_task_push_notification_config(req).await
                }
                "GetExtendedAgentCard" => client.get_extended_agent_card(req).await,
                other => {
                    return Err(tonic::Status::unimplemented(format!(
                        "unknown gRPC method: {other}"
                    )));
                }
            }
        })
        .await
        .map_err(|_| {
            trace_error!(method, "gRPC request timed out");
            ClientError::Timeout("gRPC request timed out".into())
        })?;

        match response {
            Ok(resp) => Self::decode_response(&resp.into_inner()),
            Err(status) => Err(Self::status_to_error(&status)),
        }
    }

    #[allow(clippy::significant_drop_tightening)]
    async fn execute_streaming(
        &self,
        method: &str,
        params: serde_json::Value,
        extra_headers: &HashMap<String, String>,
    ) -> ClientResult<EventStream> {
        trace_info!(
            method,
            endpoint = %self.inner.endpoint,
            "opening gRPC stream"
        );

        let payload = Self::encode_params(&params)?;
        let mut req = tonic::Request::new(payload);
        Self::add_metadata(&mut req, extra_headers);

        // FIX(#11): Per-request timeout for stream establishment, matching
        // REST/JSON-RPC stream_connect_timeout behavior.
        let stream = tokio::time::timeout(self.inner.config.timeout, async {
            let mut client = self.inner.client.lock().await;
            let response = match method {
                "SendStreamingMessage" => client.send_streaming_message(req).await,
                "SubscribeToTask" => client.subscribe_to_task(req).await,
                other => {
                    return Err(tonic::Status::unimplemented(format!(
                        "unknown streaming gRPC method: {other}"
                    )));
                }
            };
            match response {
                Ok(resp) => Ok(resp.into_inner()),
                Err(status) => Err(status),
            }
        })
        .await
        .map_err(|_| {
            trace_error!(method, "gRPC stream connect timed out");
            ClientError::Timeout("gRPC stream connect timed out".into())
        })?
        .map_err(|status| Self::status_to_error(&status))?;

        let cap = self.inner.config.stream_channel_capacity;
        let (tx, rx) = mpsc::channel::<crate::streaming::event_stream::BodyChunk>(cap);

        let task_handle = tokio::spawn(async move {
            grpc_stream_reader_task(stream, tx).await;
        });

        Ok(EventStream::with_abort_handle(
            rx,
            task_handle.abort_handle(),
        ))
    }
}

impl Transport for GrpcTransport {
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        Box::pin(self.execute_unary(method, params, extra_headers))
    }

    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(self.execute_streaming(method, params, extra_headers))
    }
}

// ── Background stream reader ────────────────────────────────────────────────

/// Reads gRPC streaming responses and feeds them to the `EventStream`
/// channel as SSE-formatted data lines. This reuses the existing SSE
/// parser in `EventStream`, matching the WebSocket transport approach.
async fn grpc_stream_reader_task(
    mut stream: tonic::Streaming<JsonPayload>,
    tx: mpsc::Sender<crate::streaming::event_stream::BodyChunk>,
) {
    use tonic::codegen::tokio_stream::StreamExt;

    loop {
        match stream.next().await {
            Some(Ok(payload)) => {
                // Each gRPC message contains raw JSON (a StreamResponse).
                // Wrap as a JSON-RPC success envelope inside an SSE frame
                // so the existing EventStream SSE parser can decode it.
                let json_str = match String::from_utf8(payload.data) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx
                            .send(Err(ClientError::Transport(format!(
                                "invalid UTF-8 in gRPC payload: {e}"
                            ))))
                            .await;
                        break;
                    }
                };
                // Wrap in JSON-RPC envelope for SSE parser compatibility.
                let envelope =
                    format!("data: {{\"jsonrpc\":\"2.0\",\"id\":null,\"result\":{json_str}}}\n\n");
                if tx
                    .send(Ok(hyper::body::Bytes::from(envelope)))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Some(Err(status)) => {
                // Use proper error code mapping instead of generic Transport
                // error, so callers can distinguish protocol errors from
                // transport issues and retry logic works correctly.
                let a2a = a2a_protocol_types::A2aError::new(
                    grpc_code_to_error_code(status.code()),
                    status.message().to_owned(),
                );
                let _ = tx.send(Err(ClientError::Protocol(a2a))).await;
                break;
            }
            None => break,
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

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

const fn grpc_code_to_error_code(code: tonic::Code) -> a2a_protocol_types::ErrorCode {
    match code {
        tonic::Code::NotFound => a2a_protocol_types::ErrorCode::TaskNotFound,
        tonic::Code::InvalidArgument
        | tonic::Code::Unauthenticated
        | tonic::Code::PermissionDenied
        | tonic::Code::ResourceExhausted => a2a_protocol_types::ErrorCode::InvalidParams,
        tonic::Code::Unimplemented => a2a_protocol_types::ErrorCode::MethodNotFound,
        tonic::Code::FailedPrecondition => a2a_protocol_types::ErrorCode::TaskNotCancelable,
        tonic::Code::DeadlineExceeded | tonic::Code::Cancelled => {
            a2a_protocol_types::ErrorCode::InternalError
        }
        _ => a2a_protocol_types::ErrorCode::InternalError,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

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
        assert!(validate_url("http://localhost:50051").is_ok());
    }

    #[test]
    fn config_default_timeout() {
        let cfg = GrpcTransportConfig::default();
        assert_eq!(cfg.timeout, Duration::from_secs(30));
    }

    #[test]
    fn config_builder() {
        let cfg = GrpcTransportConfig::default()
            .with_timeout(Duration::from_secs(60))
            .with_max_message_size(8 * 1024 * 1024)
            .with_stream_channel_capacity(128);
        assert_eq!(cfg.timeout, Duration::from_secs(60));
        assert_eq!(cfg.max_message_size, 8 * 1024 * 1024);
        assert_eq!(cfg.stream_channel_capacity, 128);
    }

    #[test]
    fn grpc_code_not_found_maps_to_task_not_found() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::NotFound),
            a2a_protocol_types::ErrorCode::TaskNotFound,
        );
    }

    #[test]
    fn grpc_code_invalid_argument_maps_to_invalid_params() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::InvalidArgument),
            a2a_protocol_types::ErrorCode::InvalidParams,
        );
    }

    #[test]
    fn grpc_code_unauthenticated_maps_to_invalid_params() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::Unauthenticated),
            a2a_protocol_types::ErrorCode::InvalidParams,
        );
    }

    #[test]
    fn grpc_code_permission_denied_maps_to_invalid_params() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::PermissionDenied),
            a2a_protocol_types::ErrorCode::InvalidParams,
        );
    }

    #[test]
    fn grpc_code_resource_exhausted_maps_to_invalid_params() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::ResourceExhausted),
            a2a_protocol_types::ErrorCode::InvalidParams,
        );
    }

    #[test]
    fn grpc_code_unimplemented_maps_to_method_not_found() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::Unimplemented),
            a2a_protocol_types::ErrorCode::MethodNotFound,
        );
    }

    #[test]
    fn grpc_code_failed_precondition_maps_to_task_not_cancelable() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::FailedPrecondition),
            a2a_protocol_types::ErrorCode::TaskNotCancelable,
        );
    }

    #[test]
    fn grpc_code_deadline_exceeded_maps_to_internal() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::DeadlineExceeded),
            a2a_protocol_types::ErrorCode::InternalError,
        );
    }

    #[test]
    fn grpc_code_cancelled_maps_to_internal() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::Cancelled),
            a2a_protocol_types::ErrorCode::InternalError,
        );
    }

    #[test]
    fn grpc_code_unknown_maps_to_internal() {
        assert_eq!(
            grpc_code_to_error_code(tonic::Code::Unknown),
            a2a_protocol_types::ErrorCode::InternalError,
        );
    }

    #[test]
    fn add_metadata_injects_a2a_version() {
        let payload = JsonPayload { data: vec![] };
        let mut req = tonic::Request::new(payload);
        let headers = HashMap::new();
        GrpcTransport::add_metadata(&mut req, &headers);
        let md = req.metadata();
        assert!(md.get("a2a-version").is_some());
        assert_eq!(
            md.get("a2a-version").unwrap().to_str().unwrap(),
            a2a_protocol_types::A2A_VERSION,
        );
    }

    #[test]
    fn add_metadata_injects_extra_headers() {
        let payload = JsonPayload { data: vec![] };
        let mut req = tonic::Request::new(payload);
        let mut headers = HashMap::new();
        headers.insert("x-custom".to_string(), "value123".to_string());
        GrpcTransport::add_metadata(&mut req, &headers);
        let md = req.metadata();
        assert_eq!(md.get("x-custom").unwrap().to_str().unwrap(), "value123",);
    }
}
