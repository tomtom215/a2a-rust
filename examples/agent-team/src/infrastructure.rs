// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Shared infrastructure: metrics, interceptors, webhook receiver, and server
//! startup helpers.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use a2a_protocol_types::error::A2aResult;

use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::interceptor::ServerInterceptor;
use a2a_protocol_server::metrics::Metrics;

use tokio::sync::Mutex;

// ── Conditional tracing macros ───────────────────────────────────────────────

#[cfg(feature = "tracing")]
macro_rules! trace_debug {
    ($($arg:tt)*) => { tracing::debug!($($arg)*) };
}
#[cfg(not(feature = "tracing"))]
macro_rules! trace_debug {
    ($($arg:tt)*) => {};
}

#[cfg(feature = "tracing")]
macro_rules! trace_warn {
    ($($arg:tt)*) => { tracing::warn!($($arg)*) };
}
#[cfg(not(feature = "tracing"))]
macro_rules! trace_warn {
    ($($arg:tt)*) => {};
}

// ── Custom metrics observer ──────────────────────────────────────────────────

/// Tracks request counts and error rates per method across all agents.
pub struct TeamMetrics {
    request_count: AtomicU64,
    response_count: AtomicU64,
    error_count: AtomicU64,
    agent_name: String,
}

impl TeamMetrics {
    pub fn new(name: &str) -> Self {
        Self {
            request_count: AtomicU64::new(0),
            response_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            agent_name: name.to_owned(),
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{}: {} requests, {} responses, {} errors",
            self.agent_name,
            self.request_count.load(Ordering::Relaxed),
            self.response_count.load(Ordering::Relaxed),
            self.error_count.load(Ordering::Relaxed),
        )
    }

    pub fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }

    pub fn error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }
}

impl Metrics for TeamMetrics {
    fn on_request(&self, _method: &str) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        trace_debug!(self.agent_name, _method, "request received");
    }

    fn on_response(&self, _method: &str) {
        self.response_count.fetch_add(1, Ordering::Relaxed);
        trace_debug!(self.agent_name, _method, "response sent");
    }

    fn on_error(&self, _method: &str, _error: &str) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        trace_warn!(self.agent_name, _method, _error, "request error");
    }

    fn on_queue_depth_change(&self, _active_queues: usize) {
        trace_debug!(self.agent_name, _active_queues, "queue depth changed");
    }
}

/// Wrapper to forward `Arc<TeamMetrics>` into the handler's `Box<dyn Metrics>`.
pub struct MetricsForward(pub Arc<TeamMetrics>);

impl Metrics for MetricsForward {
    fn on_request(&self, method: &str) {
        self.0.on_request(method);
    }
    fn on_response(&self, method: &str) {
        self.0.on_response(method);
    }
    fn on_error(&self, method: &str, error: &str) {
        self.0.on_error(method, error);
    }
    fn on_queue_depth_change(&self, active_queues: usize) {
        self.0.on_queue_depth_change(active_queues);
    }
}

// ── Audit interceptor ────────────────────────────────────────────────────────

/// Server interceptor that logs every request/response and enforces a simple
/// bearer token auth check.
pub struct AuditInterceptor {
    agent_name: String,
    expected_token: Option<String>,
}

impl AuditInterceptor {
    pub fn new(agent_name: &str) -> Self {
        Self {
            agent_name: agent_name.to_owned(),
            expected_token: None,
        }
    }

    pub fn with_token(mut self, token: &str) -> Self {
        self.expected_token = Some(token.to_owned());
        self
    }
}

impl ServerInterceptor for AuditInterceptor {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            println!(
                "  [{:>15}] INTERCEPTOR before: method={} caller={:?}",
                self.agent_name, ctx.method, ctx.caller_identity,
            );
            // If we have a required token and caller doesn't match, reject.
            if let Some(ref expected) = self.expected_token {
                if ctx.caller_identity.as_deref() != Some(expected.as_str()) {
                    // For this demo we allow through but log a warning.
                    println!(
                        "  [{:>15}] INTERCEPTOR auth warning: expected token, got {:?}",
                        self.agent_name, ctx.caller_identity,
                    );
                }
            }
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            println!(
                "  [{:>15}] INTERCEPTOR after:  method={}",
                self.agent_name, ctx.method,
            );
            Ok(())
        })
    }
}

// ── Push notification webhook receiver ───────────────────────────────────────

/// Collects push notifications delivered by agents' HttpPushSender.
#[derive(Clone)]
pub struct WebhookReceiver {
    received: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
}

impl WebhookReceiver {
    pub fn new() -> Self {
        Self {
            received: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn drain(&self) -> Vec<(String, serde_json::Value)> {
        std::mem::take(&mut *self.received.lock().await)
    }

    /// Returns the total count of received events (non-destructive).
    #[allow(dead_code)]
    pub async fn count(&self) -> usize {
        self.received.lock().await.len()
    }

    /// Returns a snapshot of all received events (non-destructive).
    pub async fn snapshot(&self) -> Vec<(String, serde_json::Value)> {
        self.received.lock().await.clone()
    }
}

/// Start a minimal HTTP server that accepts POST /webhook and records payloads.
pub async fn start_webhook_server(receiver: WebhookReceiver) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind webhook listener");
    let addr = listener.local_addr().expect("webhook addr");

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("webhook accept error (continuing): {e}");
                    continue;
                }
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let receiver = receiver.clone();
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| {
                        let receiver = receiver.clone();
                        async move {
                            // Collect body.
                            let body_bytes = http_body_util::BodyExt::collect(req.into_body())
                                .await
                                .map(|b| b.to_bytes())
                                .unwrap_or_default();

                            if let Ok(value) =
                                serde_json::from_slice::<serde_json::Value>(&body_bytes)
                            {
                                // StreamResponse serializes as {"statusUpdate":{...}}
                                // or {"artifactUpdate":{...}} (camelCase enum variant).
                                let kind = if value.get("statusUpdate").is_some() {
                                    "StatusUpdate"
                                } else if value.get("artifactUpdate").is_some() {
                                    "ArtifactUpdate"
                                } else if value.get("task").is_some() {
                                    "Task"
                                } else {
                                    "Unknown"
                                };
                                receiver
                                    .received
                                    .lock()
                                    .await
                                    .push((kind.to_owned(), value));
                            }

                            Ok::<_, std::convert::Infallible>(hyper::Response::new(
                                http_body_util::Full::new(bytes::Bytes::from("ok")),
                            ))
                        }
                    },
                );
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, svc)
                .await;
            });
        }
    });

    addr
}

// ── Server startup helpers ───────────────────────────────────────────────────

/// Pre-bind a TCP listener to an ephemeral port. Returns the listener and
/// the address it bound to, so you can construct agent cards with the correct
/// URL *before* building the handler.
pub async fn bind_listener() -> (tokio::net::TcpListener, SocketAddr) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    (listener, addr)
}

pub fn serve_jsonrpc(
    listener: tokio::net::TcpListener,
    handler: Arc<a2a_protocol_server::handler::RequestHandler>,
) {
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => continue,
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
}

/// Starts a gRPC server on a pre-bound listener.
///
/// Uses [`GrpcDispatcher::serve_with_listener`] to avoid the placeholder-URL
/// bug (Bug #12): the caller pre-binds the listener to get the address before
/// building the handler, then passes the listener here.
#[cfg(feature = "grpc")]
pub fn serve_grpc(
    listener: tokio::net::TcpListener,
    handler: Arc<a2a_protocol_server::handler::RequestHandler>,
) -> SocketAddr {
    use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};

    let config = GrpcConfig::default();
    let dispatcher = GrpcDispatcher::new(handler, config);
    dispatcher
        .serve_with_listener(listener)
        .expect("start gRPC server")
}

pub fn serve_rest(
    listener: tokio::net::TcpListener,
    handler: Arc<a2a_protocol_server::handler::RequestHandler>,
) {
    let dispatcher = Arc::new(RestDispatcher::new(handler));

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => continue,
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
}
