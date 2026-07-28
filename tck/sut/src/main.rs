// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! System Under Test (SUT) for the **official** A2A conformance suite,
//! [`a2aproject/a2a-tck`](https://github.com/a2aproject/a2a-tck).
//!
//! The TCK is language-agnostic: it discovers an agent's card over HTTP and
//! drives whichever transports the card advertises. It is not, however,
//! content-agnostic — several `MUST`-level data-model checks assert that the
//! agent emits *specific* artifact and part shapes. The TCK selects the shape
//! by prefixing the request's `messageId`, and its reference SUT
//! (`sut/a2a-python/sut_agent.py`) is the normative statement of that
//! contract. This binary implements the same contract on
//! `a2a-protocol-server`, so the official suite grades this SDK on equal
//! terms with the reference implementations.
//!
//! Run the suite against it:
//!
//! ```sh
//! cargo run --release -p a2a-tck-sut          # serves on 127.0.0.1:9999
//! ./run_tck.py --sut-host http://127.0.0.1:9999
//! ```
//!
//! Bind elsewhere with `SUT_HOST=127.0.0.1:9090`.
//!
//! ## Why this is a separate binary from `examples/echo-agent`
//!
//! The echo agent is documentation: it shows what a minimal A2A agent looks
//! like, and its behaviour should stay readable. Folding a dozen
//! `messageId`-keyed branches into it to satisfy a test harness would trade
//! that away. Keeping the SUT separate also means the conformance contract
//! lives next to the conformance tooling.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::push::{HttpPushSender, InMemoryPushConfigStore};
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

// ── The TCK's messageId contract ─────────────────────────────────────────────
//
// Mirrors `sut/a2a-python/sut_agent.py` in the a2a-tck repository. Ordering
// matters: `tck-artifact-file-url` must be tested before `tck-artifact-file`,
// and every `tck-stream-*` prefix before its non-streaming counterpart, since
// these are prefix matches.

/// Marker for the data artifact the TCK expects from `tck-artifact-data`.
const DATA_ARTIFACT: &str = r#"{"key": "value", "count": 42}"#;

struct TckSutExecutor;

impl TckSutExecutor {
    /// Emits a status update for `state`.
    async fn status(
        ctx: &RequestContext,
        queue: &dyn EventQueueWriter,
        state: TaskState,
    ) -> A2aResult<()> {
        queue
            .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                status: TaskStatus::new(state),
                metadata: None,
            }))
            .await
    }

    /// Emits a single-artifact update carrying `parts`.
    async fn artifact(
        ctx: &RequestContext,
        queue: &dyn EventQueueWriter,
        parts: Vec<Part>,
    ) -> A2aResult<()> {
        queue
            .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                artifact: Artifact::new("tck-artifact", parts),
                append: None,
                last_chunk: Some(true),
                metadata: None,
            }))
            .await
    }

    /// Emits an appendable artifact chunk (`tck-stream-artifact-chunked`).
    async fn artifact_chunk(
        ctx: &RequestContext,
        queue: &dyn EventQueueWriter,
        text: &str,
        last: bool,
    ) -> A2aResult<()> {
        queue
            .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                artifact: Artifact::new("tck-artifact", vec![Part::text(text)]),
                append: Some(true),
                last_chunk: Some(last),
                metadata: None,
            }))
            .await
    }

    /// A file part with the fixed body, media type, and filename the TCK asserts.
    fn file_part() -> Part {
        let mut part = Part::raw("dGNr"); // base64("tck")
        part.media_type = Some("text/plain".into());
        part.filename = Some("output.txt".into());
        part
    }

    /// A file-by-reference part with the URL the TCK asserts.
    fn file_url_part() -> Part {
        let mut part = Part::url("https://example.com/output.txt");
        part.media_type = Some("text/plain".into());
        part.filename = Some("output.txt".into());
        part
    }

    /// An immediate agent `Message` reply (no task), for `tck-message-response`.
    async fn message_reply(queue: &dyn EventQueueWriter, text: &str) -> A2aResult<()> {
        queue
            .write(StreamResponse::Message(Message {
                id: MessageId::new(format!("sut-{}", uuid_like(text))),
                role: MessageRole::Agent,
                parts: vec![Part::text(text)],
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            }))
            .await
    }
}

/// Deterministic pseudo-id derived from `seed` — the SUT needs a stable,
/// unique `messageId` and pulling in `uuid` for it would be overkill.
fn uuid_like(seed: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

impl AgentExecutor for TckSutExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let id = ctx.message.id.0.as_str();

            // ── Streaming behaviours ─────────────────────────────────────────
            if id.starts_with("tck-stream-artifact-chunked") {
                Self::status(ctx, queue, TaskState::Working).await?;
                Self::artifact_chunk(ctx, queue, "chunk-1 ", false).await?;
                Self::artifact_chunk(ctx, queue, "chunk-2", true).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-stream-artifact-text") {
                Self::status(ctx, queue, TaskState::Working).await?;
                Self::artifact(ctx, queue, vec![Part::text("Streamed text content")]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-stream-artifact-file") {
                Self::status(ctx, queue, TaskState::Working).await?;
                Self::artifact(ctx, queue, vec![Self::file_part()]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-stream-ordering-001") {
                Self::status(ctx, queue, TaskState::Working).await?;
                Self::artifact(ctx, queue, vec![Part::text("Ordered output")]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-stream-001") {
                Self::status(ctx, queue, TaskState::Working).await?;
                Self::artifact(ctx, queue, vec![Part::text("Stream hello from TCK")]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-stream-002") {
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-stream-003") {
                Self::status(ctx, queue, TaskState::Working).await?;
                Self::artifact(ctx, queue, vec![Part::text("Stream task lifecycle")]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }

            // ── Resubscribe: stay Working long enough to reconnect ────────────
            if id.starts_with("test-resubscribe-message-id") {
                Self::status(ctx, queue, TaskState::Working).await?;
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }

            // ── Artifact shapes (file-url before file: prefix overlap) ────────
            if id.starts_with("tck-artifact-file-url") {
                Self::artifact(ctx, queue, vec![Self::file_url_part()]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-artifact-file") {
                Self::artifact(ctx, queue, vec![Self::file_part()]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-artifact-text") {
                Self::artifact(ctx, queue, vec![Part::text("Generated text content")]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-artifact-data") {
                let value: serde_json::Value =
                    serde_json::from_str(DATA_ARTIFACT).expect("DATA_ARTIFACT is valid JSON");
                Self::artifact(ctx, queue, vec![Part::data(value)]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }

            // ── Terminal-state behaviours ────────────────────────────────────
            if id.starts_with("tck-message-response") {
                return Self::message_reply(queue, "Direct message response").await;
            }
            if id.starts_with("tck-input-required") {
                return Self::status(ctx, queue, TaskState::InputRequired).await;
            }
            if id.starts_with("tck-complete-task") {
                Self::artifact(ctx, queue, vec![Part::text("Hello from TCK")]).await?;
                return Self::status(ctx, queue, TaskState::Completed).await;
            }
            if id.starts_with("tck-reject-task") {
                return Err(A2aError::internal("rejected"));
            }

            // ── Default: echo the prefix back, as the reference SUT does ─────
            Self::artifact(
                ctx,
                queue,
                vec![Part::text(format!("Unhandled messageId prefix: {id}"))],
            )
            .await?;
            Self::status(ctx, queue, TaskState::Completed).await
        })
    }
}

// ── Agent card ───────────────────────────────────────────────────────────────

fn make_agent_card(base_url: &str) -> AgentCard {
    AgentCard {
        url: Some(base_url.into()),
        name: "a2a-rust System Under Test (SUT)".into(),
        description: "System Under Test for A2A TCK conformance".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![
            AgentInterface {
                url: base_url.into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: a2a_protocol_types::A2A_VERSION.into(),
                tenant: None,
            },
            AgentInterface {
                url: base_url.into(),
                protocol_binding: "HTTP+JSON".into(),
                protocol_version: a2a_protocol_types::A2A_VERSION.into(),
                tenant: None,
            },
        ],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "tck".into(),
            name: "TCK conformance".into(),
            description: "Emits the artifact and message shapes the A2A TCK asserts".into(),
            tags: vec!["tck".into(), "conformance".into()],
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

// ── Server ───────────────────────────────────────────────────────────────────

/// Serves JSON-RPC and HTTP+JSON on one socket.
///
/// The TCK reads both bindings' URLs from the agent card and, by default,
/// points them at the same host — so a single listener must answer both. The
/// REST dispatcher owns the routed paths (`/message:send`, `/tasks/…`) and
/// JSON-RPC owns `POST /`, which is exactly how they partition.
async fn serve(addr: SocketAddr, handler: Arc<a2a_protocol_server::handler::RequestHandler>) {
    let jsonrpc = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));
    let rest = Arc::new(RestDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind SUT listener");
    eprintln!("a2a-rust TCK SUT listening on http://{addr}");

    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let io = hyper_util::rt::TokioIo::new(stream);
        let jsonrpc = Arc::clone(&jsonrpc);
        let rest = Arc::clone(&rest);
        tokio::spawn(async move {
            let service =
                hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let jsonrpc = Arc::clone(&jsonrpc);
                    let rest = Arc::clone(&rest);
                    async move {
                        // JSON-RPC is `POST /`; everything else is REST-routed.
                        let is_jsonrpc =
                            req.method() == hyper::Method::POST && req.uri().path() == "/";
                        let resp = if is_jsonrpc {
                            jsonrpc.dispatch(req).await
                        } else {
                            rest.dispatch(req).await
                        };
                        Ok::<_, std::convert::Infallible>(resp)
                    }
                });
            let _ =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(io, service)
                    .await;
        });
    }
}

#[tokio::main]
async fn main() {
    let host = std::env::var("SUT_HOST").unwrap_or_else(|_| "127.0.0.1:9999".into());
    let addr: SocketAddr = host.parse().expect("SUT_HOST must be host:port");

    // The card's advertised URL is what the TCK actually connects to after
    // discovery. Allowing it to differ from the bind address lets the SUT run
    // behind a recording proxy, which is how the on-the-wire evidence in
    // `docs/official-tck-findings.md` was captured.
    let advertised =
        std::env::var("SUT_ADVERTISE_URL").unwrap_or_else(|_| format!("http://{addr}"));

    let handler = Arc::new(
        RequestHandlerBuilder::new(TckSutExecutor)
            .with_agent_card(make_agent_card(&advertised))
            .with_push_config_store(InMemoryPushConfigStore::new())
            // The TCK runs its webhook receiver on loopback, which the
            // sender's SSRF guard blocks by default — correct in production,
            // wrong for a conformance harness pointing at its own listener.
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .build()
            .expect("build SUT request handler"),
    );

    serve(addr, handler).await;
}
