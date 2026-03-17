// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! gRPC dispatcher for the A2A server.
//!
//! [`GrpcDispatcher`] implements the tonic-generated `A2aService` trait,
//! routing gRPC calls to the underlying [`RequestHandler`]. JSON payloads
//! are carried inside protobuf `bytes` fields, reusing the same serde types
//! as the JSON-RPC and REST bindings.
//!
//! # Configuration
//!
//! Use [`GrpcConfig`] to control message size limits and compression.
//!
//! # Example
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use a2a_protocol_server::dispatch::grpc::{GrpcDispatcher, GrpcConfig};
//! use a2a_protocol_server::RequestHandlerBuilder;
//! # struct MyExec;
//! # impl a2a_protocol_server::AgentExecutor for MyExec {
//! #     fn execute<'a>(&'a self, _: &'a a2a_protocol_server::RequestContext,
//! #         _: &'a dyn a2a_protocol_server::EventQueueWriter,
//! #     ) -> std::pin::Pin<Box<dyn std::future::Future<
//! #         Output = a2a_protocol_types::error::A2aResult<()>
//! #     > + Send + 'a>> { Box::pin(async { Ok(()) }) }
//! # }
//! # async fn example() -> std::io::Result<()> {
//! let handler = Arc::new(
//!     RequestHandlerBuilder::new(MyExec).build().unwrap()
//! );
//! let config = GrpcConfig::default();
//! let dispatcher = GrpcDispatcher::new(handler, config);
//! dispatcher.serve("127.0.0.1:50051").await?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::error::ServerError;
use crate::handler::{RequestHandler, SendMessageResult};
use crate::streaming::EventQueueReader;

// Include the generated protobuf code.
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

use proto::a2a_service_server::A2aService;
pub use proto::a2a_service_server::A2aServiceServer;
use proto::JsonPayload;

// ── GrpcConfig ──────────────────────────────────────────────────────────────

/// Configuration for the gRPC dispatcher.
///
/// Controls message size limits, compression, and concurrency settings.
///
/// # Example
///
/// ```rust
/// use a2a_protocol_server::dispatch::grpc::GrpcConfig;
///
/// let config = GrpcConfig::default()
///     .with_max_message_size(8 * 1024 * 1024)
///     .with_concurrency_limit(128);
/// ```
#[derive(Debug, Clone)]
pub struct GrpcConfig {
    /// Maximum inbound message size in bytes. Default: 4 MiB.
    pub max_message_size: usize,
    /// Maximum number of concurrent gRPC requests. Default: 256.
    pub concurrency_limit: usize,
    /// Channel capacity for streaming responses. Default: 64.
    pub stream_channel_capacity: usize,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            max_message_size: 4 * 1024 * 1024,
            concurrency_limit: 256,
            stream_channel_capacity: 64,
        }
    }
}

impl GrpcConfig {
    /// Sets the maximum inbound message size.
    #[must_use]
    pub const fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }

    /// Sets the maximum number of concurrent gRPC requests.
    #[must_use]
    pub const fn with_concurrency_limit(mut self, limit: usize) -> Self {
        self.concurrency_limit = limit;
        self
    }

    /// Sets the channel capacity for streaming responses.
    #[must_use]
    pub const fn with_stream_channel_capacity(mut self, capacity: usize) -> Self {
        self.stream_channel_capacity = capacity;
        self
    }
}

// ── GrpcDispatcher ──────────────────────────────────────────────────────────

/// gRPC dispatcher that routes A2A requests to a [`RequestHandler`].
///
/// Implements the tonic `A2aService` trait using JSON-over-gRPC payloads.
/// Create via [`GrpcDispatcher::new`] and serve with [`GrpcDispatcher::serve`]
/// or build a tonic `Router` with [`GrpcDispatcher::into_service`].
pub struct GrpcDispatcher {
    handler: Arc<RequestHandler>,
    config: GrpcConfig,
}

impl GrpcDispatcher {
    /// Creates a new gRPC dispatcher wrapping the given handler.
    #[must_use]
    pub const fn new(handler: Arc<RequestHandler>, config: GrpcConfig) -> Self {
        Self { handler, config }
    }

    /// Starts a gRPC server on the given address.
    ///
    /// Blocks until the server shuts down. Uses the configured message
    /// size limits and concurrency settings.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if binding fails.
    pub async fn serve(self, addr: impl tokio::net::ToSocketAddrs) -> std::io::Result<()> {
        let addr = resolve_addr(addr).await?;
        let svc = self.into_service();

        trace_info!(
            addr = %addr,
            "A2A gRPC server listening"
        );

        tonic::transport::Server::builder()
            .concurrency_limit_per_connection(self.config.concurrency_limit)
            .add_service(svc)
            .serve(addr)
            .await
            .map_err(std::io::Error::other)
    }

    /// Starts a gRPC server and returns the bound [`SocketAddr`].
    ///
    /// Like [`serve`](Self::serve), but returns the address immediately
    /// and runs the server in a background task. Useful for tests.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if binding fails.
    pub async fn serve_with_addr(
        self,
        addr: impl tokio::net::ToSocketAddrs,
    ) -> std::io::Result<SocketAddr> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        self.serve_with_listener(listener).await
    }

    /// Starts a gRPC server on a pre-bound [`TcpListener`](tokio::net::TcpListener).
    ///
    /// This is the recommended approach when you need to know the server
    /// address before constructing the handler (e.g., for agent cards with
    /// correct URLs). Pre-bind the listener, extract the address, build
    /// your handler, then pass the listener here.
    ///
    /// Returns the local address and runs the server in a background task.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if the listener's local address cannot be read.
    pub async fn serve_with_listener(
        self,
        listener: tokio::net::TcpListener,
    ) -> std::io::Result<SocketAddr> {
        let local_addr = listener.local_addr()?;
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        let svc = self.into_service();

        trace_info!(
            %local_addr,
            "A2A gRPC server listening"
        );

        let limit = self.config.concurrency_limit;
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .concurrency_limit_per_connection(limit)
                .add_service(svc)
                .serve_with_incoming(incoming)
                .await;
        });

        Ok(local_addr)
    }

    /// Builds the tonic service for use with a custom server setup.
    ///
    /// Returns an [`A2aServiceServer`] that can be added to a
    /// [`tonic::transport::Server`] via `add_service`.
    #[must_use]
    pub fn into_service(&self) -> A2aServiceServer<GrpcServiceImpl> {
        let inner = GrpcServiceImpl {
            handler: Arc::clone(&self.handler),
            config: self.config.clone(),
        };
        A2aServiceServer::new(inner)
            .max_decoding_message_size(self.config.max_message_size)
            .max_encoding_message_size(self.config.max_message_size)
    }
}

impl std::fmt::Debug for GrpcDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcDispatcher")
            .field("handler", &"RequestHandler { .. }")
            .field("config", &self.config)
            .finish()
    }
}

// ── GrpcServiceImpl ─────────────────────────────────────────────────────────

/// The tonic service implementation that bridges gRPC to the handler.
///
/// This type implements the generated `A2aService` trait and is not
/// typically used directly — use [`GrpcDispatcher`] instead.
pub struct GrpcServiceImpl {
    handler: Arc<RequestHandler>,
    config: GrpcConfig,
}

// ── Stream type alias ───────────────────────────────────────────────────────

/// The streaming response type for gRPC server-streaming methods.
type GrpcStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<JsonPayload, Status>> + Send + 'static>>;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Extracts gRPC metadata into a `HashMap` matching the HTTP headers
/// interface used by `RequestHandler`.
fn extract_metadata(metadata: &tonic::metadata::MetadataMap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for kv in metadata.iter() {
        if let tonic::metadata::KeyAndValueRef::Ascii(key, value) = kv {
            if let Ok(v) = value.to_str() {
                map.insert(key.as_str().to_owned(), v.to_owned());
            }
        }
    }
    map
}

/// Deserializes a JSON payload from a gRPC request.
#[allow(clippy::result_large_err)]
fn decode_json<T: serde::de::DeserializeOwned>(payload: &JsonPayload) -> Result<T, Status> {
    serde_json::from_slice(&payload.data)
        .map_err(|e| Status::invalid_argument(format!("invalid JSON payload: {e}")))
}

/// Serializes a value into a JSON payload for a gRPC response.
#[allow(clippy::result_large_err)]
fn encode_json<T: serde::Serialize>(value: &T) -> Result<JsonPayload, Status> {
    let data = serde_json::to_vec(value)
        .map_err(|e| Status::internal(format!("JSON serialization failed: {e}")))?;
    Ok(JsonPayload { data })
}

/// Converts a [`ServerError`] into a tonic [`Status`].
fn server_error_to_status(err: &ServerError) -> Status {
    let a2a_err = err.to_a2a_error();
    let code = match a2a_err.code {
        a2a_protocol_types::ErrorCode::TaskNotFound => tonic::Code::NotFound,
        a2a_protocol_types::ErrorCode::TaskNotCancelable => tonic::Code::FailedPrecondition,
        a2a_protocol_types::ErrorCode::InvalidParams
        | a2a_protocol_types::ErrorCode::ParseError => tonic::Code::InvalidArgument,
        a2a_protocol_types::ErrorCode::MethodNotFound
        | a2a_protocol_types::ErrorCode::PushNotificationNotSupported => tonic::Code::Unimplemented,
        _ => tonic::Code::Internal,
    };
    Status::new(code, a2a_err.message)
}

/// Resolves a `ToSocketAddrs` to a single `SocketAddr`.
async fn resolve_addr(addr: impl tokio::net::ToSocketAddrs) -> std::io::Result<SocketAddr> {
    tokio::net::lookup_host(addr).await?.next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "could not resolve address",
        )
    })
}

/// Converts an [`InMemoryQueueReader`] into a gRPC streaming response.
fn reader_to_grpc_stream(
    mut reader: crate::streaming::InMemoryQueueReader,
    capacity: usize,
) -> GrpcStream {
    let (tx, rx) = mpsc::channel(capacity);
    tokio::spawn(async move {
        loop {
            match reader.read().await {
                Some(Ok(event)) => {
                    let payload = match encode_json(&event) {
                        Ok(p) => p,
                        Err(status) => {
                            let _ = tx.send(Err(status)).await;
                            break;
                        }
                    };
                    if tx.send(Ok(payload)).await.is_err() {
                        break;
                    }
                }
                Some(Err(_)) => {
                    let _ = tx.send(Err(Status::internal("event queue error"))).await;
                    break;
                }
                None => break,
            }
        }
    });
    Box::pin(ReceiverStream::new(rx))
}

// ── A2aService impl ─────────────────────────────────────────────────────────

#[tonic::async_trait]
impl A2aService for GrpcServiceImpl {
    // ── Messaging ────────────────────────────────────────────────────────

    async fn send_message(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self
            .handler
            .on_send_message(params, false, Some(&headers))
            .await
        {
            Ok(SendMessageResult::Response(resp)) => Ok(Response::new(encode_json(&resp)?)),
            Ok(SendMessageResult::Stream(_)) => Err(Status::internal(
                "unexpected stream response for unary call",
            )),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    type SendStreamingMessageStream = GrpcStream;

    async fn send_streaming_message(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<Self::SendStreamingMessageStream>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self
            .handler
            .on_send_message(params, true, Some(&headers))
            .await
        {
            Ok(SendMessageResult::Stream(reader)) => {
                let stream = reader_to_grpc_stream(reader, self.config.stream_channel_capacity);
                Ok(Response::new(stream))
            }
            Ok(SendMessageResult::Response(resp)) => {
                // Wrap single response as a one-element stream.
                let payload = encode_json(&resp)?;
                let stream = Box::pin(tokio_stream::once(Ok(payload)));
                Ok(Response::new(stream as GrpcStream))
            }
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    // ── Task lifecycle ───────────────────────────────────────────────────

    async fn get_task(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self.handler.on_get_task(params, Some(&headers)).await {
            Ok(task) => Ok(Response::new(encode_json(&task)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn list_tasks(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self.handler.on_list_tasks(params, Some(&headers)).await {
            Ok(resp) => Ok(Response::new(encode_json(&resp)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn cancel_task(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self.handler.on_cancel_task(params, Some(&headers)).await {
            Ok(task) => Ok(Response::new(encode_json(&task)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    type SubscribeToTaskStream = GrpcStream;

    async fn subscribe_to_task(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<Self::SubscribeToTaskStream>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self.handler.on_resubscribe(params, Some(&headers)).await {
            Ok(reader) => {
                let stream = reader_to_grpc_stream(reader, self.config.stream_channel_capacity);
                Ok(Response::new(stream))
            }
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    // ── Push notification config ─────────────────────────────────────────

    async fn create_task_push_notification_config(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let config = decode_json(request.get_ref())?;
        match self
            .handler
            .on_set_push_config(config, Some(&headers))
            .await
        {
            Ok(cfg) => Ok(Response::new(encode_json(&cfg)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn get_task_push_notification_config(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self
            .handler
            .on_get_push_config(params, Some(&headers))
            .await
        {
            Ok(cfg) => Ok(Response::new(encode_json(&cfg)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn list_task_push_notification_configs(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params: a2a_protocol_types::params::ListPushConfigsParams =
            decode_json(request.get_ref())?;
        match self
            .handler
            .on_list_push_configs(&params.task_id, params.tenant.as_deref(), Some(&headers))
            .await
        {
            Ok(configs) => {
                let resp = a2a_protocol_types::responses::ListPushConfigsResponse {
                    configs,
                    next_page_token: None,
                };
                Ok(Response::new(encode_json(&resp)?))
            }
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn delete_task_push_notification_config(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self
            .handler
            .on_delete_push_config(params, Some(&headers))
            .await
        {
            Ok(()) => Ok(Response::new(encode_json(&serde_json::json!({}))?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    // ── Agent card ───────────────────────────────────────────────────────

    async fn get_extended_agent_card(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        match self
            .handler
            .on_get_extended_agent_card(Some(&headers))
            .await
        {
            Ok(card) => Ok(Response::new(encode_json(&card)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }
}
