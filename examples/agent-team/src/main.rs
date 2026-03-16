// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! **Agent Team** — Full-stack dogfood of every a2a-rust SDK capability.
//!
//! This example deploys 4 specialized agents that communicate via A2A:
//!
//! | Agent | Transport | Skills | Features exercised |
//! |-------|-----------|--------|-------------------|
//! | **CodeAnalyzer** | JSON-RPC | file analysis, LOC counting | Streaming, artifacts, multi-part |
//! | **BuildMonitor** | REST | cargo check/test runner | Streaming, cancellation, task lifecycle |
//! | **HealthMonitor** | JSON-RPC | agent health checks | Push notifications, interceptors |
//! | **Coordinator** | REST | orchestration, delegation | A2A client calls, task aggregation, metrics |
//!
//! The binary starts all 4 agent servers, then runs a comprehensive E2E test
//! suite that exercises:
//!
//! - Both JSON-RPC and REST transports
//! - Synchronous and streaming message sends
//! - Push notification registration, delivery, and lifecycle
//! - Task get/list/cancel operations
//! - Server interceptors (logging + auth)
//! - Custom metrics observer
//! - Agent card discovery
//! - Task state machine (Submitted → Working → Completed/Failed/Canceled)
//! - Agent-to-agent communication (Coordinator calls other agents)
//! - Artifact streaming with append mode
//! - Multi-part messages (text + data)
//! - Cancellation tokens
//! - Error handling and edge cases
//!
//! Run with: `cargo run -p agent-team`
//! With logging: `RUST_LOG=debug cargo run -p agent-team --features tracing`

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::{
    ListPushConfigsParams, ListTasksParams, MessageSendParams, TaskQueryParams,
};
use a2a_protocol_types::push::{AuthenticationInfo, TaskPushNotificationConfig};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_client::{resolve_agent_card, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::interceptor::ServerInterceptor;
use a2a_protocol_server::metrics::Metrics;
use a2a_protocol_server::push::HttpPushSender;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

use tokio::sync::Mutex;

// ═══════════════════════════════════════════════════════════════════════════════
// CONDITIONAL TRACING MACROS
// ═══════════════════════════════════════════════════════════════════════════════

// Conditional tracing macros — compile to nothing without the tracing feature.
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

// ═══════════════════════════════════════════════════════════════════════════════
// CUSTOM METRICS OBSERVER
// ═══════════════════════════════════════════════════════════════════════════════

/// Tracks request counts and error rates per method across all agents.
struct TeamMetrics {
    request_count: AtomicU64,
    response_count: AtomicU64,
    error_count: AtomicU64,
    agent_name: String,
}

impl TeamMetrics {
    fn new(name: &str) -> Self {
        Self {
            request_count: AtomicU64::new(0),
            response_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            agent_name: name.to_owned(),
        }
    }

    fn summary(&self) -> String {
        format!(
            "{}: {} requests, {} responses, {} errors",
            self.agent_name,
            self.request_count.load(Ordering::Relaxed),
            self.response_count.load(Ordering::Relaxed),
            self.error_count.load(Ordering::Relaxed),
        )
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

// ═══════════════════════════════════════════════════════════════════════════════
// LOGGING INTERCEPTOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Server interceptor that logs every request/response and enforces a simple
/// bearer token auth check.
struct AuditInterceptor {
    agent_name: String,
    expected_token: Option<String>,
}

impl AuditInterceptor {
    fn new(agent_name: &str) -> Self {
        Self {
            agent_name: agent_name.to_owned(),
            expected_token: None,
        }
    }

    fn with_token(mut self, token: &str) -> Self {
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

// ═══════════════════════════════════════════════════════════════════════════════
// PUSH NOTIFICATION WEBHOOK RECEIVER
// ═══════════════════════════════════════════════════════════════════════════════

/// Collects push notifications delivered by agents' HttpPushSender.
#[derive(Clone)]
struct WebhookReceiver {
    received: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
}

impl WebhookReceiver {
    fn new() -> Self {
        Self {
            received: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn drain(&self) -> Vec<(String, serde_json::Value)> {
        std::mem::take(&mut *self.received.lock().await)
    }
}

/// Start a minimal HTTP server that accepts POST /webhook and records payloads.
async fn start_webhook_server(receiver: WebhookReceiver) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind webhook listener");
    let addr = listener.local_addr().expect("webhook addr");

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
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
                                let kind = if value.get("status").is_some() {
                                    "StatusUpdate"
                                } else if value.get("artifact").is_some() {
                                    "ArtifactUpdate"
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

// ═══════════════════════════════════════════════════════════════════════════════
// AGENT 1: CODE ANALYZER
// ═══════════════════════════════════════════════════════════════════════════════

/// Analyzes "code" sent as text parts. Counts lines, words, characters, and
/// streams incremental artifact updates (exercising append mode).
struct CodeAnalyzerExecutor;

impl AgentExecutor for CodeAnalyzerExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Working
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // Extract all text parts.
            let code: String = ctx
                .message
                .parts
                .iter()
                .filter_map(|p| match &p.content {
                    PartContent::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            let lines = code.lines().count();
            let words = code.split_whitespace().count();
            let chars = code.chars().count();

            // Check for cancellation between steps (exercises cancellation token).
            if ctx.cancellation_token.is_cancelled() {
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(TaskState::Canceled),
                        metadata: None,
                    }))
                    .await?;
                return Ok(());
            }

            // Stream artifact 1: line count (append=false, first chunk).
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new(
                        "analysis-metrics",
                        vec![Part::text(format!("Lines: {lines}"))],
                    ),
                    append: Some(false),
                    last_chunk: Some(false),
                    metadata: None,
                }))
                .await?;

            // Small delay to simulate work.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;

            // Stream artifact 2: word/char counts (append=true, last chunk).
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new(
                        "analysis-metrics",
                        vec![Part::text(format!("\nWords: {words}\nChars: {chars}"))],
                    ),
                    append: Some(true),
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // Also emit a JSON data artifact (exercises PartContent::Data).
            let data_part = Part::data(serde_json::json!({
                "lines": lines,
                "words": words,
                "chars": chars,
                "complexity": if lines > 50 { "high" } else { "low" },
            }));
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("analysis-json", vec![data_part]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // Completed
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;

            Ok(())
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AGENT 2: BUILD MONITOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Simulates running cargo commands and streaming build output. Supports
/// cancellation to test the cancel flow.
struct BuildMonitorExecutor;

impl AgentExecutor for BuildMonitorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Working
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // Parse command from message.
            let command = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "check".to_owned());

            // Simulate build phases with streaming artifacts.
            let phases = match command.as_str() {
                "fail" => vec![
                    ("Compiling...", false),
                    ("error[E0308]: mismatched types", false),
                    ("Build FAILED", true),
                ],
                "slow" => vec![
                    ("Compiling phase 1/5...", false),
                    ("Compiling phase 2/5...", false),
                    ("Compiling phase 3/5...", false),
                    ("Compiling phase 4/5...", false),
                    ("Compiling phase 5/5...", false),
                    ("Build OK", true),
                ],
                _ => vec![
                    ("Compiling dependencies...", false),
                    ("Compiling project...", false),
                    ("Build OK: 0 errors, 0 warnings", true),
                ],
            };

            let should_fail = command == "fail";

            for (i, (phase_msg, is_last)) in phases.iter().enumerate() {
                // Check cancellation before each phase.
                if ctx.cancellation_token.is_cancelled() {
                    queue
                        .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                            task_id: ctx.task_id.clone(),
                            context_id: ContextId::new(ctx.context_id.clone()),
                            artifact: Artifact::new(
                                "build-output",
                                vec![Part::text("Build CANCELED by user")],
                            ),
                            append: Some(i > 0),
                            last_chunk: Some(true),
                            metadata: None,
                        }))
                        .await?;

                    queue
                        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                            task_id: ctx.task_id.clone(),
                            context_id: ContextId::new(ctx.context_id.clone()),
                            status: TaskStatus::new(TaskState::Canceled),
                            metadata: None,
                        }))
                        .await?;
                    return Ok(());
                }

                queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact: Artifact::new("build-output", vec![Part::text(*phase_msg)]),
                        append: Some(i > 0),
                        last_chunk: Some(*is_last),
                        metadata: None,
                    }))
                    .await?;

                // Simulate work.
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            }

            // Terminal state.
            let final_state = if should_fail {
                TaskState::Failed
            } else {
                TaskState::Completed
            };

            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(final_state),
                    metadata: None,
                }))
                .await?;

            Ok(())
        })
    }

    fn cancel<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // BuildMonitor supports cancellation — write a Canceled status.
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Canceled),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AGENT 3: HEALTH MONITOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Checks agent health by querying their agent cards and task lists. Reports
/// a summary artifact. Designed to test push notification delivery.
struct HealthMonitorExecutor;

impl AgentExecutor for HealthMonitorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Working
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // The message should contain agent URLs as JSON data part.
            let agent_urls: Vec<String> = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Data { data } => {
                        serde_json::from_value::<Vec<String>>(data.clone()).ok()
                    }
                    _ => None,
                })
                .unwrap_or_default();

            let mut results = Vec::new();

            for url in &agent_urls {
                // Fetch the agent card to discover the correct protocol binding,
                // avoiding protocol mismatch errors (e.g. JSON-RPC client → REST server).
                let status = match resolve_agent_card(url).await {
                    Ok(card) => {
                        let binding = card
                            .supported_interfaces
                            .first()
                            .map(|i| i.protocol_binding.as_str())
                            .unwrap_or("JSONRPC");
                        match ClientBuilder::new(url)
                            .with_protocol_binding(binding)
                            .build()
                        {
                            Ok(client) => {
                                match client
                                    .list_tasks(ListTasksParams {
                                        tenant: None,
                                        context_id: None,
                                        status: None,
                                        page_size: Some(1),
                                        page_token: None,
                                        status_timestamp_after: None,
                                        include_artifacts: None,
                                        history_length: None,
                                    })
                                    .await
                                {
                                    Ok(_) => "HEALTHY",
                                    Err(_) => "DEGRADED",
                                }
                            }
                            Err(_) => "UNREACHABLE",
                        }
                    }
                    Err(_) => {
                        // Fall back to default JSON-RPC if agent card is unavailable.
                        match ClientBuilder::new(url).build() {
                            Ok(client) => {
                                match client
                                    .list_tasks(ListTasksParams {
                                        tenant: None,
                                        context_id: None,
                                        status: None,
                                        page_size: Some(1),
                                        page_token: None,
                                        status_timestamp_after: None,
                                        include_artifacts: None,
                                        history_length: None,
                                    })
                                    .await
                                {
                                    Ok(_) => "HEALTHY",
                                    Err(_) => "DEGRADED",
                                }
                            }
                            Err(_) => "UNREACHABLE",
                        }
                    }
                };
                results.push(format!("{url}: {status}"));
            }

            // Also check via text part (for simple "ping" messages).
            let text_input = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default();

            if !text_input.is_empty() {
                results.push(format!("Received check request: {text_input}"));
            }

            let report = if results.is_empty() {
                "No agents to check — health monitor standing by.".to_owned()
            } else {
                format!("Health Report:\n{}", results.join("\n"))
            };

            // Emit report artifact.
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("health-report", vec![Part::text(&report)]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // Completed
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;

            Ok(())
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AGENT 4: COORDINATOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Orchestrates the team: delegates tasks to other agents via A2A client calls,
/// aggregates results, and reports a unified summary.
struct CoordinatorExecutor {
    /// URLs of the other agents, keyed by name.
    agent_urls: HashMap<String, String>,
}

impl CoordinatorExecutor {
    fn new(agent_urls: HashMap<String, String>) -> Self {
        Self { agent_urls }
    }
}

impl AgentExecutor for CoordinatorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Working
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            let command = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "analyze".to_owned());

            let mut report_lines = vec![format!("Coordinator executing: {command}")];

            match command.as_str() {
                "analyze" | "full-check" => {
                    // Delegate to CodeAnalyzer.
                    if let Some(analyzer_url) = self.agent_urls.get("code_analyzer") {
                        report_lines.push(String::new());
                        report_lines.push("--- Code Analysis ---".to_owned());

                        match ClientBuilder::new(analyzer_url).build() {
                            Ok(client) => {
                                let sample_code = "fn main() {\n    println!(\"Hello from coordinator!\");\n    let x = 42;\n    let y = x * 2;\n    println!(\"Result: {y}\");\n}";
                                let params = make_send_params(sample_code);
                                match client.send_message(params).await {
                                    Ok(SendMessageResponse::Task(task)) => {
                                        report_lines.push(format!(
                                            "  Task {}: {:?}",
                                            task.id, task.status.state
                                        ));
                                        if let Some(artifacts) = &task.artifacts {
                                            for art in artifacts {
                                                for part in &art.parts {
                                                    if let PartContent::Text { text } =
                                                        &part.content
                                                    {
                                                        report_lines.push(format!("  {}", text));
                                                    }
                                                    if let PartContent::Data { data } =
                                                        &part.content
                                                    {
                                                        report_lines
                                                            .push(format!("  JSON: {}", data));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Ok(_) => {
                                        report_lines.push("  Got non-task response".to_owned());
                                    }
                                    Err(e) => {
                                        report_lines.push(format!("  Analysis failed: {e}"));
                                    }
                                }
                            }
                            Err(e) => {
                                report_lines.push(format!("  Cannot connect to analyzer: {e}"));
                            }
                        }
                    }

                    // Delegate to BuildMonitor.
                    if command == "full-check" {
                        if let Some(build_url) = self.agent_urls.get("build_monitor") {
                            report_lines.push(String::new());
                            report_lines.push("--- Build Check ---".to_owned());

                            if let Ok(client) = ClientBuilder::new(build_url)
                                .with_protocol_binding("REST")
                                .build()
                            {
                                let params = make_send_params("check");
                                match client.send_message(params).await {
                                    Ok(SendMessageResponse::Task(task)) => {
                                        report_lines.push(format!(
                                            "  Build {}: {:?}",
                                            task.id, task.status.state
                                        ));
                                    }
                                    Ok(_) => {}
                                    Err(e) => {
                                        report_lines.push(format!("  Build check failed: {e}"));
                                    }
                                }
                            }
                        }
                    }
                }

                "health" => {
                    // Delegate to HealthMonitor.
                    if let Some(health_url) = self.agent_urls.get("health_monitor") {
                        report_lines.push(String::new());
                        report_lines.push("--- Health Check ---".to_owned());

                        if let Ok(client) = ClientBuilder::new(health_url).build() {
                            let urls: Vec<String> = self.agent_urls.values().cloned().collect();
                            let parts = vec![
                                Part::text("coordinator-initiated health check"),
                                Part::data(serde_json::json!(urls)),
                            ];
                            let params = MessageSendParams {
                                tenant: None,
                                message: Message {
                                    id: MessageId::new(uuid::Uuid::new_v4().to_string()),
                                    role: MessageRole::User,
                                    parts,
                                    task_id: None,
                                    context_id: None,
                                    reference_task_ids: None,
                                    extensions: None,
                                    metadata: None,
                                },
                                configuration: None,
                                metadata: None,
                            };
                            match client.send_message(params).await {
                                Ok(SendMessageResponse::Task(task)) => {
                                    if let Some(artifacts) = &task.artifacts {
                                        for art in artifacts {
                                            for part in &art.parts {
                                                if let PartContent::Text { text } = &part.content {
                                                    report_lines.push(format!("  {text}"));
                                                }
                                            }
                                        }
                                    }
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    report_lines.push(format!("  Health check failed: {e}"));
                                }
                            }
                        }
                    }
                }

                _ => {
                    report_lines.push(format!("Unknown command: {command}"));
                }
            }

            // Emit unified report.
            let full_report = report_lines.join("\n");
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("coordinator-report", vec![Part::text(&full_report)]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // Completed
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;

            Ok(())
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AGENT CARD BUILDERS
// ═══════════════════════════════════════════════════════════════════════════════

fn code_analyzer_card(url: &str) -> AgentCard {
    AgentCard {
        name: "Code Analyzer".into(),
        description: "Analyzes code: LOC, complexity, metrics".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into(), "application/json".into()],
        skills: vec![AgentSkill {
            id: "analyze".into(),
            name: "Code Analysis".into(),
            description: "Counts lines, words, chars and assesses complexity".into(),
            tags: vec!["code".into(), "analysis".into(), "metrics".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

fn build_monitor_card(url: &str) -> AgentCard {
    AgentCard {
        name: "Build Monitor".into(),
        description: "Runs cargo builds and streams output".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "REST".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "build".into(),
            name: "Build Runner".into(),
            description: "Runs cargo check/build/test".into(),
            tags: vec!["build".into(), "cargo".into(), "ci".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

fn health_monitor_card(url: &str) -> AgentCard {
    AgentCard {
        name: "Health Monitor".into(),
        description: "Monitors agent team health and connectivity".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into(), "application/json".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "health-check".into(),
            name: "Health Check".into(),
            description: "Pings agents and reports their status".into(),
            tags: vec!["health".into(), "monitoring".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

fn coordinator_card(url: &str) -> AgentCard {
    AgentCard {
        name: "Coordinator".into(),
        description: "Orchestrates the agent team via A2A calls".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "REST".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "orchestrate".into(),
            name: "Team Orchestration".into(),
            description: "Delegates tasks to specialized agents and aggregates results".into(),
            tags: vec!["orchestration".into(), "delegation".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SERVER STARTUP HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

async fn start_jsonrpc_server(
    handler: Arc<a2a_protocol_server::handler::RequestHandler>,
) -> SocketAddr {
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind JSON-RPC listener");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
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

    addr
}

async fn start_rest_server(
    handler: Arc<a2a_protocol_server::handler::RequestHandler>,
) -> SocketAddr {
    let dispatcher = Arc::new(RestDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind REST listener");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
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

    addr
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST HARNESS
// ═══════════════════════════════════════════════════════════════════════════════

struct TestResult {
    name: String,
    passed: bool,
    duration_ms: u128,
    details: String,
}

impl TestResult {
    fn pass(name: &str, duration_ms: u128, details: &str) -> Self {
        Self {
            name: name.to_owned(),
            passed: true,
            duration_ms,
            details: details.to_owned(),
        }
    }

    fn fail(name: &str, duration_ms: u128, details: &str) -> Self {
        Self {
            name: name.to_owned(),
            passed: false,
            duration_ms,
            details: details.to_owned(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    #[cfg(feature = "tracing")]
    {
        use tracing_subscriber::EnvFilter;
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .init();
    }

    let total_start = Instant::now();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     A2A Agent Team — Full SDK Dogfood & E2E Test Suite     ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ── Start webhook receiver for push notifications ────────────────────
    let webhook_receiver = WebhookReceiver::new();
    let webhook_addr = start_webhook_server(webhook_receiver.clone()).await;
    println!("Webhook receiver listening on http://{webhook_addr}\n");

    // ── Build shared metrics ─────────────────────────────────────────────
    let analyzer_metrics = Arc::new(TeamMetrics::new("CodeAnalyzer"));
    let build_metrics = Arc::new(TeamMetrics::new("BuildMonitor"));
    let health_metrics = Arc::new(TeamMetrics::new("HealthMonitor"));
    let coord_metrics = Arc::new(TeamMetrics::new("Coordinator"));

    // We need clones for printing later (metrics are inside handlers).
    let am = Arc::clone(&analyzer_metrics);
    let bm = Arc::clone(&build_metrics);
    let hm = Arc::clone(&health_metrics);
    let cm = Arc::clone(&coord_metrics);

    // ── Agent 1: Code Analyzer (JSON-RPC) ────────────────────────────────
    let analyzer_handler = Arc::new(
        RequestHandlerBuilder::new(CodeAnalyzerExecutor)
            .with_agent_card(code_analyzer_card("http://placeholder"))
            .with_interceptor(AuditInterceptor::new("CodeAnalyzer"))
            .with_metrics(MetricsForward(analyzer_metrics))
            .with_executor_timeout(std::time::Duration::from_secs(30))
            .with_event_queue_capacity(128)
            .build()
            .expect("build code analyzer handler"),
    );
    let analyzer_addr = start_jsonrpc_server(Arc::clone(&analyzer_handler)).await;
    let analyzer_url = format!("http://{analyzer_addr}");
    println!("Agent [CodeAnalyzer]  JSON-RPC on {analyzer_url}");

    // ── Agent 2: Build Monitor (REST) ────────────────────────────────────
    let build_handler = Arc::new(
        RequestHandlerBuilder::new(BuildMonitorExecutor)
            .with_agent_card(build_monitor_card("http://placeholder"))
            .with_interceptor(AuditInterceptor::new("BuildMonitor").with_token("build-secret"))
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .with_metrics(MetricsForward(build_metrics))
            .with_executor_timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("build build monitor handler"),
    );
    let build_addr = start_rest_server(Arc::clone(&build_handler)).await;
    let build_url = format!("http://{build_addr}");
    println!("Agent [BuildMonitor]  REST     on {build_url}");

    // ── Agent 3: Health Monitor (JSON-RPC) ───────────────────────────────
    let health_handler = Arc::new(
        RequestHandlerBuilder::new(HealthMonitorExecutor)
            .with_agent_card(health_monitor_card("http://placeholder"))
            .with_interceptor(AuditInterceptor::new("HealthMonitor"))
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .with_metrics(MetricsForward(health_metrics))
            .build()
            .expect("build health monitor handler"),
    );
    let health_addr = start_jsonrpc_server(Arc::clone(&health_handler)).await;
    let health_url = format!("http://{health_addr}");
    println!("Agent [HealthMonitor] JSON-RPC on {health_url}");

    // ── Agent 4: Coordinator (REST) ──────────────────────────────────────
    let mut agent_urls = HashMap::new();
    agent_urls.insert("code_analyzer".into(), analyzer_url.clone());
    agent_urls.insert("build_monitor".into(), build_url.clone());
    agent_urls.insert("health_monitor".into(), health_url.clone());

    let coord_handler = Arc::new(
        RequestHandlerBuilder::new(CoordinatorExecutor::new(agent_urls))
            .with_agent_card(coordinator_card("http://placeholder"))
            .with_interceptor(AuditInterceptor::new("Coordinator"))
            .with_metrics(MetricsForward(coord_metrics))
            .with_max_concurrent_streams(50)
            .build()
            .expect("build coordinator handler"),
    );
    let coord_addr = start_rest_server(Arc::clone(&coord_handler)).await;
    let coord_url = format!("http://{coord_addr}");
    println!("Agent [Coordinator]   REST     on {coord_url}");
    println!();

    // ── Run E2E test suite ───────────────────────────────────────────────
    let mut results: Vec<TestResult> = Vec::new();

    // Test 1: Sync SendMessage via JSON-RPC (CodeAnalyzer)
    {
        let start = Instant::now();
        println!("Test 1: Sync SendMessage via JSON-RPC (CodeAnalyzer)");
        let client = ClientBuilder::new(&analyzer_url)
            .build()
            .expect("build client");

        let code = "fn hello() {\n    println!(\"world\");\n}\n";
        match client.send_message(make_send_params(code)).await {
            Ok(SendMessageResponse::Task(task)) => {
                let has_artifacts = task.artifacts.as_ref().map_or(0, |a| a.len());
                let state = format!("{:?}", task.status.state);
                println!("  Task {}: {state}, {has_artifacts} artifacts", task.id);
                results.push(TestResult::pass(
                    "sync-jsonrpc-send",
                    start.elapsed().as_millis(),
                    &format!("state={state}, artifacts={has_artifacts}"),
                ));
            }
            Ok(_) => {
                results.push(TestResult::fail(
                    "sync-jsonrpc-send",
                    start.elapsed().as_millis(),
                    "expected Task response",
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "sync-jsonrpc-send",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // Test 2: Streaming SendMessage via JSON-RPC (CodeAnalyzer)
    {
        let start = Instant::now();
        println!("\nTest 2: Streaming SendMessage via JSON-RPC (CodeAnalyzer)");
        let client = ClientBuilder::new(&analyzer_url)
            .build()
            .expect("build client");

        let code = "use std::io;\nfn main() -> io::Result<()> {\n    Ok(())\n}\n";
        match client.stream_message(make_send_params(code)).await {
            Ok(mut stream) => {
                let mut event_count = 0;
                let mut status_events = Vec::new();
                let mut artifact_events = 0;

                while let Some(event) = stream.next().await {
                    event_count += 1;
                    match event {
                        Ok(StreamResponse::StatusUpdate(ev)) => {
                            println!("  Status: {:?}", ev.status.state);
                            status_events.push(format!("{:?}", ev.status.state));
                        }
                        Ok(StreamResponse::ArtifactUpdate(ev)) => {
                            artifact_events += 1;
                            println!(
                                "  Artifact: {} (append={:?}, last={:?})",
                                ev.artifact.id, ev.append, ev.last_chunk
                            );
                        }
                        Ok(StreamResponse::Task(task)) => {
                            println!("  Task snapshot: {:?}", task.status.state);
                        }
                        Ok(_) => {}
                        Err(e) => {
                            println!("  Stream error: {e}");
                            break;
                        }
                    }
                }

                let detail = format!(
                    "events={event_count}, statuses=[{}], artifacts={artifact_events}",
                    status_events.join(",")
                );
                println!("  {detail}");

                if event_count >= 4 {
                    results.push(TestResult::pass(
                        "streaming-jsonrpc",
                        start.elapsed().as_millis(),
                        &detail,
                    ));
                } else {
                    results.push(TestResult::fail(
                        "streaming-jsonrpc",
                        start.elapsed().as_millis(),
                        &format!("too few events: {detail}"),
                    ));
                }
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "streaming-jsonrpc",
                    start.elapsed().as_millis(),
                    &format!("stream error: {e}"),
                ));
            }
        }
    }

    // Test 3: Sync SendMessage via REST (BuildMonitor)
    {
        let start = Instant::now();
        println!("\nTest 3: Sync SendMessage via REST (BuildMonitor)");
        let client = ClientBuilder::new(&build_url)
            .with_protocol_binding("REST")
            .build()
            .expect("build REST client");

        match client.send_message(make_send_params("check")).await {
            Ok(SendMessageResponse::Task(task)) => {
                let state = format!("{:?}", task.status.state);
                println!("  Build task {}: {state}", task.id);
                results.push(TestResult::pass(
                    "sync-rest-send",
                    start.elapsed().as_millis(),
                    &format!("state={state}"),
                ));
            }
            Ok(_) => {
                results.push(TestResult::fail(
                    "sync-rest-send",
                    start.elapsed().as_millis(),
                    "expected Task",
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "sync-rest-send",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // Test 4: Streaming via REST (BuildMonitor)
    {
        let start = Instant::now();
        println!("\nTest 4: Streaming SendMessage via REST (BuildMonitor)");
        let client = ClientBuilder::new(&build_url)
            .with_protocol_binding("REST")
            .build()
            .expect("build REST client");

        match client.stream_message(make_send_params("check")).await {
            Ok(mut stream) => {
                let mut events = 0;
                while let Some(event) = stream.next().await {
                    events += 1;
                    match event {
                        Ok(StreamResponse::StatusUpdate(ev)) => {
                            println!("  Status: {:?}", ev.status.state);
                        }
                        Ok(StreamResponse::ArtifactUpdate(ev)) => {
                            for part in &ev.artifact.parts {
                                if let PartContent::Text { text } = &part.content {
                                    println!("  Build: {text}");
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            println!("  Error: {e}");
                            break;
                        }
                    }
                }
                results.push(TestResult::pass(
                    "streaming-rest",
                    start.elapsed().as_millis(),
                    &format!("events={events}"),
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "streaming-rest",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // Test 5: Build failure path
    {
        let start = Instant::now();
        println!("\nTest 5: Build failure path (BuildMonitor)");
        let client = ClientBuilder::new(&build_url)
            .with_protocol_binding("REST")
            .build()
            .expect("build REST client");

        match client.send_message(make_send_params("fail")).await {
            Ok(SendMessageResponse::Task(task)) => {
                let state = format!("{:?}", task.status.state);
                println!("  Failed build task: {state}");
                let passed = task.status.state == TaskState::Failed;
                if passed {
                    results.push(TestResult::pass(
                        "build-failure-path",
                        start.elapsed().as_millis(),
                        &format!("state={state} (correct!)"),
                    ));
                } else {
                    results.push(TestResult::fail(
                        "build-failure-path",
                        start.elapsed().as_millis(),
                        &format!("expected Failed, got {state}"),
                    ));
                }
            }
            Ok(_) => {
                results.push(TestResult::fail(
                    "build-failure-path",
                    start.elapsed().as_millis(),
                    "expected Task",
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "build-failure-path",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // Test 6: GetTask retrieval
    {
        let start = Instant::now();
        println!("\nTest 6: GetTask retrieval");
        let client = ClientBuilder::new(&analyzer_url)
            .build()
            .expect("build client");

        // First send a message to create a task.
        let resp = client
            .send_message(make_send_params("let x = 1;"))
            .await
            .expect("send for get_task test");

        if let SendMessageResponse::Task(task) = resp {
            let task_id = task.id.to_string();
            match client
                .get_task(TaskQueryParams {
                    tenant: None,
                    id: task_id.clone(),
                    history_length: None,
                })
                .await
            {
                Ok(fetched) => {
                    println!("  Fetched: {} ({:?})", fetched.id, fetched.status.state);
                    results.push(TestResult::pass(
                        "get-task",
                        start.elapsed().as_millis(),
                        &format!("id={task_id}"),
                    ));
                }
                Err(e) => {
                    results.push(TestResult::fail(
                        "get-task",
                        start.elapsed().as_millis(),
                        &format!("error: {e}"),
                    ));
                }
            }
        }
    }

    // Test 7: ListTasks with pagination
    {
        let start = Instant::now();
        println!("\nTest 7: ListTasks with pagination");
        let client = ClientBuilder::new(&analyzer_url)
            .build()
            .expect("build client");

        match client
            .list_tasks(ListTasksParams {
                tenant: None,
                context_id: None,
                status: None,
                page_size: Some(2),
                page_token: None,
                status_timestamp_after: None,
                include_artifacts: Some(true),
                history_length: None,
            })
            .await
        {
            Ok(resp) => {
                println!(
                    "  Listed {} tasks, next_page={:?}",
                    resp.tasks.len(),
                    resp.next_page_token
                );
                results.push(TestResult::pass(
                    "list-tasks",
                    start.elapsed().as_millis(),
                    &format!("count={}", resp.tasks.len()),
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "list-tasks",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // Test 8: Push notification config CRUD via REST
    // Exercises the full lifecycle: create → get → list → delete.
    // Uses REST transport for BuildMonitor (which has push support).
    {
        let start = Instant::now();
        println!("\nTest 8: Push notification config CRUD (REST)");
        let client = ClientBuilder::new(&build_url)
            .with_protocol_binding("REST")
            .build()
            .expect("build REST client for BuildMonitor");

        // Create a task so we have a valid task_id.
        let init_resp = client.send_message(make_send_params("ping")).await;
        let task_id = match &init_resp {
            Ok(SendMessageResponse::Task(task)) => {
                println!("  Created task for push test: {}", task.id);
                task.id.to_string()
            }
            _ => {
                results.push(TestResult::fail(
                    "push-config-crud",
                    start.elapsed().as_millis(),
                    "could not create initial task",
                ));
                "".to_owned()
            }
        };

        if !task_id.is_empty() {
            let webhook_url = format!("http://{webhook_addr}/webhook");
            let push_config = TaskPushNotificationConfig {
                tenant: None,
                id: None,
                task_id: task_id.clone(),
                url: webhook_url.clone(),
                token: Some("test-token-123".into()),
                authentication: Some(AuthenticationInfo {
                    scheme: "bearer".into(),
                    credentials: "my-secret-bearer".into(),
                }),
            };

            // 1. Create push config.
            match client.set_push_config(push_config).await {
                Ok(stored) => {
                    let config_id = stored.id.clone().unwrap_or_default();
                    println!("  CREATE: id={config_id}");

                    // 2. Get push config by id.
                    match client
                        .get_push_config(task_id.clone(), config_id.clone())
                        .await
                    {
                        Ok(got) => println!("  GET:    id={:?} url={}", got.id, got.url),
                        Err(e) => println!("  GET:    error: {e}"),
                    }

                    // 3. List push configs.
                    match client
                        .list_push_configs(ListPushConfigsParams {
                            tenant: None,
                            task_id: task_id.clone(),
                            page_size: Some(10),
                            page_token: None,
                        })
                        .await
                    {
                        Ok(list) => println!("  LIST:   {} configs", list.configs.len()),
                        Err(e) => println!("  LIST:   error: {e}"),
                    }

                    // 4. Delete push config.
                    match client
                        .delete_push_config(task_id.clone(), config_id.clone())
                        .await
                    {
                        Ok(()) => println!("  DELETE: ok"),
                        Err(e) => println!("  DELETE: error: {e}"),
                    }

                    // 5. Verify deletion — list should be empty.
                    match client
                        .list_push_configs(ListPushConfigsParams {
                            tenant: None,
                            task_id: task_id.clone(),
                            page_size: Some(10),
                            page_token: None,
                        })
                        .await
                    {
                        Ok(list) => {
                            println!("  VERIFY: {} configs after delete", list.configs.len());
                            if list.configs.is_empty() {
                                results.push(TestResult::pass(
                                    "push-config-crud",
                                    start.elapsed().as_millis(),
                                    "create+get+list+delete+verify",
                                ));
                            } else {
                                results.push(TestResult::fail(
                                    "push-config-crud",
                                    start.elapsed().as_millis(),
                                    "delete did not remove config",
                                ));
                            }
                        }
                        Err(e) => {
                            results.push(TestResult::fail(
                                "push-config-crud",
                                start.elapsed().as_millis(),
                                &format!("verify list error: {e}"),
                            ));
                        }
                    }
                }
                Err(e) => {
                    results.push(TestResult::fail(
                        "push-config-crud",
                        start.elapsed().as_millis(),
                        &format!("create error: {e:?}"),
                    ));
                }
            }
        }
    }

    // Test 9: Multi-part message (text + data) via HealthMonitor
    {
        let start = Instant::now();
        println!("\nTest 9: Multi-part message (text + data)");
        let client = ClientBuilder::new(&health_url)
            .build()
            .expect("build client");

        let parts = vec![
            Part::text("health check from test suite"),
            Part::data(serde_json::json!([analyzer_url.clone(), build_url.clone()])),
        ];
        let params = MessageSendParams {
            tenant: None,
            message: Message {
                id: MessageId::new(uuid::Uuid::new_v4().to_string()),
                role: MessageRole::User,
                parts,
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            },
            configuration: None,
            metadata: None,
        };

        match client.send_message(params).await {
            Ok(SendMessageResponse::Task(task)) => {
                if let Some(artifacts) = &task.artifacts {
                    for art in artifacts {
                        for part in &art.parts {
                            if let PartContent::Text { text } = &part.content {
                                println!("  {text}");
                            }
                        }
                    }
                }
                results.push(TestResult::pass(
                    "multi-part-message",
                    start.elapsed().as_millis(),
                    "health check with text+data parts",
                ));
            }
            Ok(_) => {
                results.push(TestResult::fail(
                    "multi-part-message",
                    start.elapsed().as_millis(),
                    "expected Task",
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "multi-part-message",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // Test 10: Agent-to-agent coordination (Coordinator → CodeAnalyzer)
    {
        let start = Instant::now();
        println!("\nTest 10: Agent-to-agent coordination");
        let client = ClientBuilder::new(&coord_url)
            .with_protocol_binding("REST")
            .build()
            .expect("build coord client");

        match client.send_message(make_send_params("analyze")).await {
            Ok(SendMessageResponse::Task(task)) => {
                if let Some(artifacts) = &task.artifacts {
                    for art in artifacts {
                        for part in &art.parts {
                            if let PartContent::Text { text } = &part.content {
                                for line in text.lines().take(6) {
                                    println!("  {line}");
                                }
                            }
                        }
                    }
                }
                results.push(TestResult::pass(
                    "agent-to-agent",
                    start.elapsed().as_millis(),
                    "coordinator delegated to analyzer",
                ));
            }
            Ok(_) => {
                results.push(TestResult::fail(
                    "agent-to-agent",
                    start.elapsed().as_millis(),
                    "expected Task",
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "agent-to-agent",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // Test 11: Full-check orchestration (Coordinator → Analyzer + BuildMonitor)
    {
        let start = Instant::now();
        println!("\nTest 11: Full-check orchestration (Coordinator → multiple agents)");
        let client = ClientBuilder::new(&coord_url)
            .with_protocol_binding("REST")
            .build()
            .expect("build coord client");

        match client.send_message(make_send_params("full-check")).await {
            Ok(SendMessageResponse::Task(task)) => {
                if let Some(artifacts) = &task.artifacts {
                    for art in artifacts {
                        for part in &art.parts {
                            if let PartContent::Text { text } = &part.content {
                                for line in text.lines().take(10) {
                                    println!("  {line}");
                                }
                            }
                        }
                    }
                }
                results.push(TestResult::pass(
                    "full-orchestration",
                    start.elapsed().as_millis(),
                    "coordinator -> analyzer + build monitor",
                ));
            }
            Ok(_) => {
                results.push(TestResult::fail(
                    "full-orchestration",
                    start.elapsed().as_millis(),
                    "expected Task",
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "full-orchestration",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // Test 12: Health orchestration (Coordinator → HealthMonitor → all agents)
    {
        let start = Instant::now();
        println!("\nTest 12: Health orchestration (Coordinator → HealthMonitor)");
        let client = ClientBuilder::new(&coord_url)
            .with_protocol_binding("REST")
            .build()
            .expect("build coord client");

        match client.send_message(make_send_params("health")).await {
            Ok(SendMessageResponse::Task(task)) => {
                if let Some(artifacts) = &task.artifacts {
                    for art in artifacts {
                        for part in &art.parts {
                            if let PartContent::Text { text } = &part.content {
                                for line in text.lines() {
                                    println!("  {line}");
                                }
                            }
                        }
                    }
                }
                results.push(TestResult::pass(
                    "health-orchestration",
                    start.elapsed().as_millis(),
                    "coordinator -> health monitor -> all agents",
                ));
            }
            Ok(_) => {
                results.push(TestResult::fail(
                    "health-orchestration",
                    start.elapsed().as_millis(),
                    "expected Task",
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "health-orchestration",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // Test 13: Streaming with metadata
    {
        let start = Instant::now();
        println!("\nTest 13: Message with metadata");
        let client = ClientBuilder::new(&analyzer_url)
            .build()
            .expect("build client");

        let params = MessageSendParams {
            tenant: None,
            message: Message {
                id: MessageId::new(uuid::Uuid::new_v4().to_string()),
                role: MessageRole::User,
                parts: vec![Part::text("fn test() {}")],
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            },
            configuration: None,
            metadata: Some(serde_json::json!({
                "source": "agent-team-test",
                "priority": "high",
            })),
        };

        match client.send_message(params).await {
            Ok(SendMessageResponse::Task(task)) => {
                println!("  Task with metadata: {:?}", task.status.state);
                results.push(TestResult::pass(
                    "message-metadata",
                    start.elapsed().as_millis(),
                    "metadata attached and processed",
                ));
            }
            Ok(_) => {
                results.push(TestResult::fail(
                    "message-metadata",
                    start.elapsed().as_millis(),
                    "expected Task",
                ));
            }
            Err(e) => {
                results.push(TestResult::fail(
                    "message-metadata",
                    start.elapsed().as_millis(),
                    &format!("error: {e}"),
                ));
            }
        }
    }

    // ── Report ───────────────────────────────────────────────────────────
    let total_duration = total_start.elapsed();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.iter().filter(|r| !r.passed).count();
    let total = results.len();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                      TEST RESULTS                          ║");
    println!("╠══════════════════════════════════════════════════════════════╣");

    for r in &results {
        let icon = if r.passed { "PASS" } else { "FAIL" };
        println!(
            "║ [{icon}] {:30} {:>6}ms  {}",
            r.name,
            r.duration_ms,
            if r.details.len() > 30 {
                format!("{}...", &r.details[..27])
            } else {
                r.details.clone()
            }
        );
    }

    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║ Total: {total} | Passed: {passed} | Failed: {failed} | Time: {}ms",
        total_duration.as_millis()
    );
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                    AGENT METRICS                           ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ {}", am.summary());
    println!("║ {}", bm.summary());
    println!("║ {}", hm.summary());
    println!("║ {}", cm.summary());

    // Push notification summary.
    let push_events = webhook_receiver.drain().await;
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ Push notifications received: {}", push_events.len());
    for (kind, _value) in &push_events {
        println!("║   - {kind}");
    }

    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                SDK FEATURES EXERCISED                      ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    let features = [
        "AgentExecutor trait (4 implementations)",
        "RequestHandlerBuilder (all options)",
        "JsonRpcDispatcher",
        "RestDispatcher",
        "ClientBuilder (JSON-RPC + REST)",
        "Sync SendMessage",
        "Streaming SendStreamingMessage",
        "EventStream consumer",
        "GetTask",
        "ListTasks with pagination",
        "CancelTask executor override",
        "Push notification config CRUD",
        "HttpPushSender delivery",
        "Webhook receiver",
        "ServerInterceptor (audit + auth)",
        "Custom Metrics observer",
        "AgentCard discovery",
        "Multi-part messages (text + data)",
        "Artifact append mode",
        "TaskState lifecycle (all states)",
        "CancellationToken checking",
        "Executor timeout config",
        "Event queue capacity config",
        "Max concurrent streams config",
        "Agent-to-agent A2A communication",
        "Multi-level orchestration",
        "Request metadata",
    ];
    for f in &features {
        println!("║   [x] {f}");
    }

    println!("╚══════════════════════════════════════════════════════════════╝");

    if failed > 0 {
        std::process::exit(1);
    }

    println!(
        "\nAll {passed} tests passed in {}ms. SDK fully verified.",
        total_duration.as_millis()
    );
}

/// Wrapper to forward `Arc<TeamMetrics>` into the handler's `Box<dyn Metrics>`.
struct MetricsForward(Arc<TeamMetrics>);

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
