// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Incident-response agent team — what agents actually are, hands-on.
//!
//! A wrapped prompt answers once and forgets. An agent holds a **task**: it
//! gathers evidence with tools, delegates to other agents, pauses to ask the
//! caller for missing information, streams progress while it works, can be
//! cancelled mid-flight, and ends in an honest terminal state. This example
//! demonstrates every one of those properties with three cooperating A2A
//! agents you can run on a laptop:
//!
//! | Agent     | Port | Brain        | Job |
//! |-----------|------|--------------|-----|
//! | `triage`  | 9200 | LLM + tools  | Orchestrates an incident: collects evidence from the other two agents, asks for missing info (`INPUT_REQUIRED`), synthesizes an incident report |
//! | `logs`    | 9201 | none (deterministic) | Searches the service log for a service's lines — proof that an "agent" is a capability behind the protocol, not necessarily an LLM |
//! | `runbook` | 9202 | LLM (optional) | Returns the operational runbook for a service, AI-summarized when a model is available, verbatim otherwise |
//!
//! Run `cargo run -p incident-response` for a narrated end-to-end demo, or
//! start each role separately (`-- triage|logs|runbook`) and drive them with
//! any A2A client. See the README for the local-model setup (llama-server /
//! Ollama on `:11434`) — the demo also works with no model at all, falling
//! back to mechanical summaries so the protocol mechanics stay visible.

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_example_harness::{counter, sweep, Binding, Matrix};
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::handler::RequestHandler;
use a2a_protocol_server::push::{HttpPushSender, InMemoryPushConfigStore};
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

/// Bundled at compile time so the demo has zero filesystem setup.
const INCIDENT_LOG: &str = include_str!("../data/incident.log");
const RUNBOOKS: &str = include_str!("../data/runbooks.md");

const TRIAGE_PORT: u16 = 9200;
const LOGS_PORT: u16 = 9201;
const RUNBOOK_PORT: u16 = 9202;

// ── Small shared helpers ─────────────────────────────────────────────────────

fn extract_text(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|p| match &p.content {
            PartContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn agent_message(text: &str) -> Message {
    Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::Agent,
        parts: vec![Part::text(text)],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

fn user_message(text: &str) -> Message {
    Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::User,
        parts: vec![Part::text(text)],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

fn send_params(message: Message) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message,
        configuration: None,
        metadata: None,
    }
}

/// Emits a Working status carrying a human-readable progress note.
///
/// Status messages are how an agent narrates long-running work — a streaming
/// client sees these the moment they happen.
async fn progress(queue: &dyn EventQueueWriter, ctx: &RequestContext, note: &str) -> A2aResult<()> {
    queue
        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            status: TaskStatus {
                state: TaskState::Working,
                message: Some(agent_message(note)),
                timestamp: None,
            },
            metadata: None,
        }))
        .await
}

async fn emit_artifact(
    queue: &dyn EventQueueWriter,
    ctx: &RequestContext,
    name: &str,
    text: &str,
) -> A2aResult<()> {
    queue
        .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            artifact: Artifact::new(name, vec![Part::text(text)]),
            append: None,
            last_chunk: Some(true),
            metadata: None,
        }))
        .await
}

async fn complete(queue: &dyn EventQueueWriter, ctx: &RequestContext) -> A2aResult<()> {
    queue
        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            status: TaskStatus::new(TaskState::Completed),
            metadata: None,
        }))
        .await
}

/// Names of the services that have a runbook section (`## <service>`).
fn known_services() -> Vec<&'static str> {
    RUNBOOKS
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(str::trim)
        .collect()
}

/// Finds the first known service mentioned in `text`, if any.
fn find_service(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    known_services().into_iter().find(|s| lower.contains(*s))
}

/// Optional LLM completion via genai. Model names that don't match a hosted
/// provider route to the local Ollama-compatible endpoint on `:11434`.
/// Returns `None` (rather than failing the task) when no model is reachable,
/// so the team degrades to mechanical summaries instead of going dark.
async fn try_llm(client: &genai::Client, model: &str, prompt: &str) -> Option<String> {
    let req = genai::chat::ChatRequest::new(vec![
        genai::chat::ChatMessage::system(
            "You are a concise SRE assistant. Answer in plain text, no markdown headers.",
        ),
        genai::chat::ChatMessage::user(prompt),
    ]);
    let resp = tokio::time::timeout(Duration::from_secs(60), client.exec_chat(model, req, None))
        .await
        .ok()?
        .ok()?;
    resp.first_text().map(str::to_string)
}

fn incident_model() -> String {
    std::env::var("INCIDENT_MODEL").unwrap_or_else(|_| "qwen3-0.6b".to_string())
}

// ── Agent 1: logs — a deterministic tool agent (no LLM) ─────────────────────

/// Searches the bundled service log for lines mentioning a service.
///
/// Deliberately not LLM-backed: an A2A agent is a *capability behind the
/// protocol*. Tools with exact semantics (log search, database lookups,
/// deployments) should stay deterministic; the judgment lives in the agents
/// that call them.
struct LogSearchExecutor;

impl AgentExecutor for LogSearchExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let query = extract_text(&ctx.message.parts);
            let service = find_service(&query).ok_or_else(|| {
                A2aError::invalid_params(format!(
                    "no known service in query; known services: {}",
                    known_services().join(", ")
                ))
            })?;

            progress(queue, ctx, &format!("searching log for '{service}'")).await?;

            let matches: Vec<&str> = INCIDENT_LOG
                .lines()
                .filter(|l| l.contains(service))
                .collect();
            let errors = matches.iter().filter(|l| l.contains("ERROR")).count();
            let warns = matches.iter().filter(|l| l.contains("WARN")).count();

            let report = format!(
                "{} lines for '{service}' ({errors} ERROR, {warns} WARN):\n{}",
                matches.len(),
                matches.join("\n"),
            );
            emit_artifact(queue, ctx, "log-findings", &report).await?;
            complete(queue, ctx).await
        })
    }
}

// ── Agent 2: runbook — LLM-assisted with graceful degradation ───────────────

/// Returns the runbook section for a service, AI-summarized when a model is
/// reachable and verbatim when not — degraded output is labeled, never
/// silent.
struct RunbookExecutor {
    client: genai::Client,
    model: String,
}

impl AgentExecutor for RunbookExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let query = extract_text(&ctx.message.parts);
            let service = find_service(&query).ok_or_else(|| {
                A2aError::invalid_params(format!(
                    "no known service in query; known services: {}",
                    known_services().join(", ")
                ))
            })?;

            // Slice this service's section out of the bundled runbook file.
            let marker = format!("## {service}");
            let start = RUNBOOKS
                .find(&marker)
                .ok_or_else(|| A2aError::internal("runbook section disappeared"))?;
            let rest = &RUNBOOKS[start + marker.len()..];
            let end = rest.find("\n## ").map_or(rest.len(), |i| i);
            let section = rest[..end].trim();

            progress(queue, ctx, &format!("loading runbook for '{service}'")).await?;

            let guidance = match try_llm(
                &self.client,
                &self.model,
                &format!(
                    "Summarize this runbook into at most 3 imperative remediation \
                     steps for an on-call engineer:\n\n{section}"
                ),
            )
            .await
            {
                Some(summary) => format!("[AI summary — model: {}]\n{summary}", self.model),
                None => format!("[verbatim runbook — no model reachable]\n{section}"),
            };

            emit_artifact(queue, ctx, "runbook-guidance", &guidance).await?;
            complete(queue, ctx).await
        })
    }
}

// ── Agent 3: triage — the orchestrator ───────────────────────────────────────

/// Coordinates an incident: pauses the task for missing input, delegates to
/// the logs and runbook agents over real A2A client calls, and synthesizes an
/// incident report.
struct TriageExecutor {
    client: genai::Client,
    model: String,
    logs_url: String,
    runbook_url: String,
    /// Alerts parked in `INPUT_REQUIRED`, keyed by task id. The original
    /// alert is also recoverable from `ctx.stored_task` history; keeping it
    /// here shows that agents are allowed to hold working state across the
    /// turns of one task.
    pending: Mutex<HashMap<String, String>>,
}

impl TriageExecutor {
    /// Delegates one question to a specialist agent and returns the first
    /// artifact's text. Failures are reported inline so a missing specialist
    /// degrades the report instead of killing the incident response.
    async fn ask_specialist(&self, url: &str, what: &str, question: &str) -> String {
        let client = match ClientBuilder::new(url).build() {
            Ok(c) => c,
            Err(e) => return format!("[{what} agent unavailable: {e}]"),
        };
        match tokio::time::timeout(
            Duration::from_secs(90),
            client.send_message(send_params(user_message(question))),
        )
        .await
        {
            Ok(Ok(SendMessageResponse::Task(task))) => task
                .artifacts
                .as_deref()
                .and_then(<[Artifact]>::first)
                .map(|a| extract_text(&a.parts))
                .unwrap_or_else(|| format!("[{what} agent returned no artifact]")),
            Ok(Ok(_)) => format!("[{what} agent returned an unexpected response type]"),
            Ok(Err(e)) => format!("[{what} agent error: {e}]"),
            Err(_) => format!("[{what} agent timed out]"),
        }
    }
}

impl AgentExecutor for TriageExecutor {
    /// Cancellation is cooperative in A2A: the default implementation
    /// reports tasks as not cancelable, and executors opt in by overriding
    /// `cancel` to release whatever the task holds. Triage holds parked
    /// alerts, so cancel drops them; the server then records the Canceled
    /// state.
    fn cancel<'a>(
        &'a self,
        ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.pending.lock().unwrap().remove(&ctx.task_id.0);
            Ok(())
        })
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let incoming = extract_text(&ctx.message.parts);

            // Multi-turn: if this task previously parked in INPUT_REQUIRED,
            // this message is the answer — recombine it with the original
            // alert. This is the part a wrapped prompt cannot do: the task
            // id carries state across turns.
            let parked = self.pending.lock().unwrap().remove(&ctx.task_id.0);
            let alert = match parked {
                Some(original) => format!("{original}\n(operator follow-up: {incoming})"),
                None => incoming,
            };

            // The A2A state machine requires SUBMITTED → WORKING before any
            // interrupted state, so announce work before deciding to ask.
            progress(queue, ctx, "reading alert").await?;

            // Do we know which service this is about? If not, *ask* — the
            // task parks in INPUT_REQUIRED and resumes when the caller sends
            // the answer on the same task id.
            let Some(service) = find_service(&alert) else {
                self.pending
                    .lock()
                    .unwrap()
                    .insert(ctx.task_id.0.clone(), alert);
                return queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus {
                            state: TaskState::InputRequired,
                            message: Some(agent_message(&format!(
                                "Which service is affected? I have runbooks for: {}",
                                known_services().join(", ")
                            ))),
                            timestamp: None,
                        },
                        metadata: None,
                    }))
                    .await;
            };

            // Evidence gathering: real A2A calls to the specialist agents.
            progress(
                queue,
                ctx,
                &format!("triaging '{service}': querying log agent"),
            )
            .await?;
            let log_findings = self
                .ask_specialist(&self.logs_url, "log", &format!("service={service}"))
                .await;

            progress(queue, ctx, "querying runbook agent").await?;
            let runbook = self
                .ask_specialist(&self.runbook_url, "runbook", &format!("service={service}"))
                .await;

            // Synthesis: LLM if reachable, labeled mechanical fallback if not.
            progress(queue, ctx, "synthesizing incident report").await?;
            let synthesis = try_llm(
                &self.client,
                &self.model,
                &format!(
                    "Write a short incident report (3-6 sentences): likely root \
                     cause and next actions.\n\nAlert: {alert}\n\nLog findings:\n\
                     {log_findings}\n\nRunbook guidance:\n{runbook}"
                ),
            )
            .await
            .unwrap_or_else(|| {
                "[mechanical summary — no model reachable]\nSee the log findings and \
                 runbook guidance below; follow the runbook steps in order."
                    .to_string()
            });

            let report = format!(
                "INCIDENT REPORT — service: {service}\n\
                 ================================{pad}\n\n\
                 Alert:\n{alert}\n\n\
                 Assessment:\n{synthesis}\n\n\
                 Evidence (log agent):\n{log_findings}\n\n\
                 Guidance (runbook agent):\n{runbook}\n",
                pad = "=".repeat(service.len()),
            );
            emit_artifact(queue, ctx, "incident-report", &report).await?;
            complete(queue, ctx).await
        })
    }
}

// ── Server scaffolding ───────────────────────────────────────────────────────

fn make_card(
    url: &str,
    grpc: &str,
    ws: &str,
    name: &str,
    description: &str,
    skill: &str,
) -> AgentCard {
    AgentCard {
        url: Some(url.into()),
        name: name.into(),
        description: description.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        supported_interfaces: vec![
            AgentInterface {
                url: url.into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: a2a_protocol_types::A2A_VERSION.into(),
                tenant: None,
            },
            // The same socket answers REST: JSON-RPC owns POST `/`, the REST
            // dispatcher owns the `/v1/...` paths.
            AgentInterface {
                url: url.into(),
                protocol_binding: "HTTP+JSON".into(),
                protocol_version: a2a_protocol_types::A2A_VERSION.into(),
                tenant: None,
            },
            AgentInterface {
                url: grpc.into(),
                protocol_binding: "GRPC".into(),
                protocol_version: a2a_protocol_types::A2A_VERSION.into(),
                tenant: None,
            },
            AgentInterface {
                url: ws.into(),
                protocol_binding: "WEBSOCKET".into(),
                protocol_version: a2a_protocol_types::A2A_VERSION.into(),
                tenant: None,
            },
        ],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: skill.into(),
            name: skill.into(),
            description: description.into(),
            tags: vec!["incident-response".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        // `extended_agent_card` joins the other two because the surface
        // sweep drives `GetExtendedAgentCard`, and the server answers
        // `UnsupportedOperation` for a card that does not advertise it
        // (spec §3.1.11) — an undriven method and an unavailable one look the
        // same from the client, and only one of them is this example's fault.
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true)
            .with_extended_agent_card(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// Where one agent is reachable, per binding.
#[derive(Clone)]
pub struct AgentEndpoints {
    /// JSON-RPC and HTTP+JSON share this socket.
    pub http: String,
    /// gRPC, as `host:port`.
    pub grpc: String,
    /// WebSocket, as a `ws://` URL.
    pub ws: String,
}

/// Binds `port` (plus two derived ports) and serves `executor` on all four
/// bindings.
///
/// Ports are `port`, `port + 10` for gRPC and `port + 20` for WebSocket, so a
/// reader starting a role by hand can predict them. Until 2026-08-11 every
/// agent here spoke JSON-RPC only, which meant three of the four transports
/// this SDK ships had no representation in the example that `examples/README`
/// tells people to start with.
async fn start_agent(
    port: u16,
    name: &str,
    description: &str,
    skill: &str,
    executor: impl AgentExecutor,
) -> Result<AgentEndpoints, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let addr: SocketAddr = listener.local_addr()?;
    let url = format!("http://{addr}");

    let grpc_listener = tokio::net::TcpListener::bind(("127.0.0.1", port + 10)).await?;
    let grpc_addr: SocketAddr = grpc_listener.local_addr()?;
    let ws_bind = format!("127.0.0.1:{}", port + 20);

    let endpoints = AgentEndpoints {
        http: url.clone(),
        grpc: grpc_addr.to_string(),
        ws: format!("ws://{ws_bind}"),
    };

    let handler: Arc<RequestHandler> = Arc::new(
        RequestHandlerBuilder::new(executor)
            .with_agent_card(make_card(
                &url,
                &endpoints.grpc,
                &endpoints.ws,
                name,
                description,
                skill,
            ))
            .with_push_config_store(InMemoryPushConfigStore::new())
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            // Spec §13.3 wants the extended card authenticated; this demo
            // ships no authenticator, so the opt-in is explicit. The refusal
            // path is covered by the counter-tests, which use a separate agent
            // that has neither.
            .allow_unauthenticated_extended_card()
            .build()?,
    );

    // JSON-RPC and REST on one socket, routed by request shape.
    let jsonrpc = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));
    let rest = Arc::new(a2a_protocol_server::dispatch::RestDispatcher::new(
        Arc::clone(&handler),
    ));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let jsonrpc = Arc::clone(&jsonrpc);
            let rest = Arc::clone(&rest);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                    let jsonrpc = Arc::clone(&jsonrpc);
                    let rest = Arc::clone(&rest);
                    async move {
                        let is_jsonrpc = req.method() == hyper::Method::POST
                            && (req.uri().path() == "/" || req.uri().path().is_empty());
                        if is_jsonrpc {
                            Ok::<_, std::convert::Infallible>(jsonrpc.dispatch(req).await)
                        } else {
                            Ok::<_, std::convert::Infallible>(rest.dispatch(req).await)
                        }
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

    {
        use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};
        GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default())
            .serve_with_listener(grpc_listener)?;
    }
    {
        let ws = Arc::new(
            a2a_protocol_server::dispatch::websocket::WebSocketDispatcher::new(Arc::clone(
                &handler,
            )),
        );
        ws.serve_with_addr(ws_bind.as_str()).await?;
    }

    Ok(endpoints)
}

async fn start_logs_agent() -> Result<AgentEndpoints, Box<dyn std::error::Error>> {
    start_agent(
        LOGS_PORT,
        "Log Search Agent",
        "Deterministic log search over the service log (no LLM)",
        "log-search",
        LogSearchExecutor,
    )
    .await
}

async fn start_runbook_agent() -> Result<AgentEndpoints, Box<dyn std::error::Error>> {
    start_agent(
        RUNBOOK_PORT,
        "Runbook Agent",
        "Serves per-service runbook guidance, AI-summarized when a model is available",
        "runbook-lookup",
        RunbookExecutor {
            client: genai::Client::default(),
            model: incident_model(),
        },
    )
    .await
}

async fn start_triage_agent(
    logs_url: String,
    runbook_url: String,
) -> Result<AgentEndpoints, Box<dyn std::error::Error>> {
    start_agent(
        TRIAGE_PORT,
        "Incident Triage Agent",
        "Orchestrates incident triage: gathers evidence from specialist agents and produces an incident report",
        "incident-triage",
        TriageExecutor {
            client: genai::Client::default(),
            model: incident_model(),
            logs_url,
            runbook_url,
            pending: Mutex::new(HashMap::new()),
        },
    )
    .await
}

/// Message-text marker that leaves a triage task parked in `INPUT_REQUIRED`.
///
/// Any alert naming no known service parks awaiting input — that is Act 1's
/// whole point — so the surface sweep reuses it to obtain a non-terminal task
/// for `SubscribeToTask`.
const SURFACE_PAUSE_PREFIX: &str = "vague alert, no service named: ";

/// A webhook sink so push configs point at something that answers.
///
/// A config accepted against a dead URL proves storage, not delivery.
async fn start_webhook_sink() -> Result<String, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(|_req| async {
                    Ok::<_, std::convert::Infallible>(hyper::Response::new(
                        http_body_util::Full::new(bytes::Bytes::from_static(b"ok")),
                    ))
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });
    Ok(format!("http://{addr}/webhook"))
}

/// Builds a client speaking `binding` against `ep`.
async fn build_client(
    binding: Binding,
    ep: &AgentEndpoints,
) -> Result<a2a_protocol_client::A2aClient, String> {
    match binding {
        Binding::JsonRpc => ClientBuilder::new(&ep.http)
            .build()
            .map_err(|e| e.to_string()),
        Binding::HttpJson => ClientBuilder::new(&ep.http)
            .with_protocol_binding("HTTP+JSON")
            .build()
            .map_err(|e| e.to_string()),
        Binding::Grpc => {
            let url = format!("http://{}", ep.grpc);
            let t = a2a_protocol_client::transport::grpc::GrpcTransport::connect(&url)
                .await
                .map_err(|e| format!("gRPC connect: {e}"))?;
            ClientBuilder::new(&url)
                .with_custom_transport(t)
                .without_tls()
                .build()
                .map_err(|e| e.to_string())
        }
        Binding::WebSocket => {
            let t = a2a_protocol_client::transport::WebSocketTransport::connect(ep.ws.clone())
                .await
                .map_err(|e| format!("WebSocket connect: {e}"))?;
            ClientBuilder::new(&ep.ws)
                .with_custom_transport(t)
                .without_tls()
                .build()
                .map_err(|e| e.to_string())
        }
    }
}

/// An agent advertising no optional capabilities, for the counter-tests.
async fn start_restricted_agent() -> Result<String, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr: SocketAddr = listener.local_addr()?;
    let url = format!("http://{addr}");

    let mut card = make_card(
        &url,
        "",
        "",
        "Restricted Agent",
        "no optional capabilities",
        "none",
    );
    card.capabilities = AgentCapabilities::none();
    card.supported_interfaces.truncate(1);

    let handler: Arc<RequestHandler> = Arc::new(
        RequestHandlerBuilder::new(LogSearchExecutor)
            .with_agent_card(card)
            .build()?,
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let d = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&d);
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
    Ok(url)
}

// ── Demo client ──────────────────────────────────────────────────────────────

/// Streams one message to the triage agent, narrating every event, and
/// returns the task id, context id, and last observed state.
async fn stream_and_narrate(
    client: &a2a_protocol_client::A2aClient,
    message: Message,
) -> Result<(Option<String>, Option<String>, Option<TaskState>), Box<dyn std::error::Error>> {
    let mut stream = client.stream_message(send_params(message)).await?;
    let mut task_id = None;
    let mut context_id = None;
    let mut last_state = None;

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamResponse::Task(task)) => {
                println!("  ⇢ task {} [{}]", task.id.0, task.status.state);
                task_id = Some(task.id.0);
                context_id = Some(task.context_id.0.clone());
                last_state = Some(task.status.state);
            }
            Ok(StreamResponse::StatusUpdate(ev)) => {
                let note = ev
                    .status
                    .message
                    .as_ref()
                    .map(|m| format!(" — {}", extract_text(&m.parts)))
                    .unwrap_or_default();
                println!("  ⇢ status: {}{note}", ev.status.state);
                last_state = Some(ev.status.state);
            }
            Ok(StreamResponse::ArtifactUpdate(ev)) => {
                println!(
                    "  ⇢ artifact '{}' ({} chars)",
                    ev.artifact.name.as_deref().unwrap_or(&ev.artifact.id.0),
                    extract_text(&ev.artifact.parts).len()
                );
            }
            Ok(_) => {}
            Err(e) => println!("  ⇢ stream error: {e}"),
        }
    }
    Ok((task_id, context_id, last_state))
}

#[allow(clippy::too_many_lines)]
async fn run_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("Incident-Response Agent Team");
    println!("============================");
    println!();
    println!(
        "Model: {} (set INCIDENT_MODEL to change; with no model running",
        incident_model()
    );
    println!("on :11434 the agents fall back to labeled mechanical summaries)");
    println!();

    let logs = start_logs_agent().await?;
    let runbook = start_runbook_agent().await?;
    let triage = start_triage_agent(logs.http.clone(), runbook.http.clone()).await?;
    for (label, ep) in [("logs", &logs), ("runbook", &runbook), ("triage", &triage)] {
        println!("{label:<8} agent: {}  grpc {}  {}", ep.http, ep.grpc, ep.ws);
    }
    let triage_url = triage.http.clone();

    let client = ClientBuilder::new(&triage_url).build()?;

    // ── Act 1: a vague alert — the agent asks instead of guessing ─────────
    println!();
    println!("ACT 1 — vague alert: the task pauses and asks for missing input");
    println!("  → \"Customers report payments failing since 14:00, please investigate\"");
    let (task_id, context_id, state) = stream_and_narrate(
        &client,
        user_message("Customers report payments failing since 14:00, please investigate"),
    )
    .await?;
    let task_id = task_id.ok_or("no task id from stream")?;
    let context_id = context_id.ok_or("no context id from stream")?;
    assert_eq!(
        state,
        Some(TaskState::InputRequired),
        "expected the task to park in INPUT_REQUIRED"
    );

    // ── Act 2: answer on the SAME task — it resumes where it left off ─────
    println!();
    println!("ACT 2 — the operator answers on the same task; the agents collaborate");
    println!("  → \"it's payments-api\"  (task {task_id})");
    // Continuing a task requires BOTH ids: the task id selects the parked
    // task, and the context id must match the conversation it belongs to.
    let mut follow_up = user_message("it's payments-api");
    follow_up.task_id = Some(TaskId(task_id.clone()));
    follow_up.context_id = Some(ContextId::new(context_id));
    let (_, _, state) = stream_and_narrate(&client, follow_up).await?;
    assert_eq!(state, Some(TaskState::Completed), "triage should complete");

    // Fetch the finished task and print the report artifact.
    let task = client
        .get_task(a2a_protocol_types::params::TaskQueryParams {
            tenant: None,
            id: task_id.clone(),
            history_length: None,
        })
        .await?;
    if let Some(report) = task
        .artifacts
        .as_deref()
        .and_then(<[Artifact]>::first)
        .map(|a| extract_text(&a.parts))
    {
        println!();
        println!("─── incident-report artifact ───");
        println!("{report}");
        println!("────────────────────────────────");
    }

    // ── Act 3: cancellation — a paused task can be called off ─────────────
    // A vague alert parks the task in INPUT_REQUIRED; instead of answering,
    // the operator decides it was noise and cancels it. (Cancelling mid-WORK
    // also works, but with a warm local model the whole triage can finish in
    // under a second — a pause is the deterministic place to demonstrate it.)
    println!();
    println!("ACT 3 — tasks are cancellable: the operator calls off a parked task");
    println!("  → \"seeing odd error rates somewhere, look into it\"");
    let (cancel_id, _, state) = stream_and_narrate(
        &client,
        user_message("seeing odd error rates somewhere, look into it"),
    )
    .await?;
    let cancel_id = cancel_id.ok_or("no task id for cancel demo")?;
    assert_eq!(state, Some(TaskState::InputRequired));
    let canceled = client.cancel_task(cancel_id.clone()).await?;
    println!("  → cancel_task(...) ⇒ {}", canceled.status.state);
    assert_eq!(canceled.status.state, TaskState::Canceled);

    // ── Act 4: the whole protocol surface, measured ───────────────────────
    //
    // Acts 1-3 show what an agent *is*. They do not show that this SDK serves
    // the whole A2A surface, and until 2026-08-11 this example drove 4 of the
    // 11 methods over 1 of the 4 bindings while `examples/README` presented it
    // as the place to start. That gap was invisible because nothing counted.
    //
    // A "vague alert" is the marker that parks a task in INPUT_REQUIRED, which
    // is what `SubscribeToTask` needs: the server refuses to re-attach to a
    // terminal task, correctly, so without a non-terminal one the success path
    // is unreachable and only the refusal is ever observed. Act 1 already
    // relies on this behaviour, so the sweep reuses the example's own
    // semantics rather than inventing a sleep.
    println!();
    println!("ACT 4 — every A2A method over every binding, counted");
    let webhook = start_webhook_sink().await?;
    let mut matrix = Matrix::new();
    let mut failures: Vec<String> = Vec::new();

    for binding in Binding::ALL {
        let surface_client = match build_client(*binding, &triage).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("could not build a {} client: {e}", binding.label());
                std::process::exit(1);
            }
        };
        println!("  --- {} ---", binding.label());
        let outcome = sweep(
            &surface_client,
            *binding,
            &webhook,
            SURFACE_PAUSE_PREFIX,
            &mut matrix,
        )
        .await;
        for l in &outcome.lines {
            println!("  {l}");
        }
        failures.extend(outcome.failures);
    }

    // Counter-tests need an agent that advertises nothing optional; one agent
    // cannot both support and refuse a capability.
    println!("  --- counter-tests (calls that must be refused) ---");
    let restricted = start_restricted_agent().await?;
    let counter_out = counter::run(
        &ClientBuilder::new(&triage_url).build()?,
        &ClientBuilder::new(&restricted).build()?,
    )
    .await;
    for l in &counter_out.lines {
        println!("  {l}");
    }
    failures.extend(counter_out.failures);

    println!();
    let missing = matrix.report();
    if !failures.is_empty() {
        println!("\n{} call(s) failed:", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        std::process::exit(1);
    }
    if !missing.is_empty() {
        println!("\n{} matrix cell(s) never ran:", missing.len());
        for (m, b) in &missing {
            println!("  - {} over {}", m.wire_name(), b.label());
        }
        std::process::exit(2);
    }
    println!();
    println!("Every A2A method was exercised over every binding, and every");
    println!("counter-test was refused as the specification requires.");

    println!();
    // `INCIDENT_EXIT_WHEN_DONE=1` returns instead of parking on Ctrl+C, so CI
    // can gate on the exit code. Without it the demo stays up for a human to
    // poke at, which is the point of the three agents still serving.
    if std::env::var("INCIDENT_EXIT_WHEN_DONE").is_ok() {
        println!("Done (INCIDENT_EXIT_WHEN_DONE set — exiting).");
        return Ok(());
    }
    println!("Done. The three agents are still serving — probe them with curl or");
    println!("the TCK (cargo run -p a2a-tck -- --url {triage_url}),");
    println!("or press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let role = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "demo".to_string());
    match role.as_str() {
        "demo" => run_demo().await,
        "logs" => {
            let ep = start_logs_agent().await?;
            println!("logs agent listening on {}", ep.http);
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        "runbook" => {
            let ep = start_runbook_agent().await?;
            println!("runbook agent listening on {}", ep.http);
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        "triage" => {
            let ep = start_triage_agent(
                format!("http://127.0.0.1:{LOGS_PORT}"),
                format!("http://127.0.0.1:{RUNBOOK_PORT}"),
            )
            .await?;
            println!("triage agent listening on {}", ep.http);
            println!("(expects logs on :{LOGS_PORT} and runbook on :{RUNBOOK_PORT})");
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        other => {
            eprintln!("unknown role '{other}' — use: demo | triage | logs | runbook");
            std::process::exit(2);
        }
    }
}
