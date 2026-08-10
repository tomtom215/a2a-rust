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

/// Which capability set the SUT advertises.
///
/// Several MUST requirements are only *reachable* when the agent advertises
/// something other than the full capability set, and one SUT cannot exercise
/// every side at once — so the profile is selectable and CI runs the suite
/// once per profile:
///
/// * [`Profile::Minimal`] advertises **less**. `CORE-CAP-001/002/003` check
///   that a server rejects streaming, push and extended-card operations it
///   never claimed to support, and the TCK skips them against an agent that
///   does claim them.
/// * [`Profile::Extension`] advertises a **required extension**, which is the
///   only way `CORE-CAP-004` becomes observable at all.
///
/// Verified against `reports/compatibility.json` from both runs: the three
/// requirements the minimal profile adds are exactly `CORE-CAP-001`,
/// `CORE-CAP-002` and `CORE-CAP-003`. `CORE-CAP-004` is `SKIPPED` under both
/// Full and Minimal, because neither card declares the sentinel extension its
/// test requires.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Profile {
    /// Everything on — the profile the conformance gate runs against.
    Full,
    /// Streaming, push and extended card all absent, so the capability
    /// *rejection* paths become observable.
    Minimal,
    /// Full capabilities plus [`REQUIRED_EXTENSION_URI`] declared
    /// `required: true`, so `CORE-CAP-004`'s precondition is met.
    ///
    /// This profile is only usable for a **scoped** run — see that constant's
    /// documentation for why.
    Extension,

    /// Full capabilities, plus a card that **declares** a bearer scheme and
    /// **enforces** it on every binding.
    ///
    /// This profile exists for exactly one requirement: `BIND-EQUIV-004`'s
    /// enforcement half. §5.1 asks that every binding support the same
    /// authentication schemes, and half of that is checkable from the card
    /// alone — v1.0 has no per-interface security field, so the schemes are
    /// shared by construction. The other half is behavioural: *do all four
    /// bindings actually reject an uncredentialed caller, and accept a
    /// credentialed one?* No card can answer that, and against a target with
    /// no credentials to withhold the question cannot even be posed. Until
    /// this profile existed, that half was recorded as unmeasured.
    ///
    /// Enforcement is a single [`BearerTokenAuthInterceptor`] on the
    /// `RequestHandler`, which is the point: interceptors run above the
    /// dispatchers, so JSON-RPC, HTTP+JSON, gRPC and WebSocket are guarded by
    /// one implementation reading one [`CallContext`]. That is the design the
    /// check is verifying, so the check must be able to fail if a binding ever
    /// stops populating those headers — which is the failure mode a
    /// per-binding auth implementation would produce.
    ///
    /// Deliberately **not** used for the official suite: the harness has no
    /// credential to present, so every check would fail on authentication and
    /// grade nothing. Only the in-repo runner's `--equivalence` mode drives it.
    Secured,
}

/// The bearer token [`Profile::Secured`] accepts.
///
/// A fixed constant rather than an environment variable because both sides of
/// the probe need it — the SUT to accept it and the TCK to present it — and a
/// mismatch would make the check fail for a reason that has nothing to do with
/// binding equivalence. It is a conformance fixture, not a secret; the profile
/// is never used outside a test harness driving loopback.
pub const SECURED_PROFILE_TOKEN: &str = "tck-bind-equiv-004-token";

/// The name under which [`Profile::Secured`] declares its bearer scheme.
pub const SECURED_PROFILE_SCHEME: &str = "tckBearer";

impl Profile {
    fn from_env() -> Self {
        match std::env::var("SUT_PROFILE").as_deref() {
            Ok("minimal") => Self::Minimal,
            Ok("extension") => Self::Extension,
            Ok("secured") => Self::Secured,
            _ => Self::Full,
        }
    }
}

/// The sentinel extension URI `CORE-CAP-004` looks for on the agent card.
///
/// The test only runs when the card declares this URI with `required: true`;
/// otherwise it records `SKIPPED`. It then sends an ordinary `SendMessage`
/// *without* an `A2A-Extensions` declaration and requires
/// `ExtensionSupportRequiredError` back.
///
/// # Why this profile must be run scoped to `CORE-CAP-004`
///
/// Spec §3.3.4 says required-extension enforcement applies to every request,
/// and this SDK implements it that way (`ensure_required_extensions` is called
/// from `messaging.rs`, `get_task.rs`, `list_tasks.rs`, `cancel_task.rs`,
/// `subscribe.rs` and `push_config.rs`). The TCK does not send `A2A-Extensions`
/// activation on its ordinary positive requests — upstream
/// [`a2aproject/a2a-tck` #193](https://github.com/a2aproject/a2a-tck/issues/193),
/// still open — so under this profile every other `CORE-*` check would be
/// answered with `ExtensionSupportRequiredError` and fail.
///
/// Those requirements are graded by the full-profile run instead. Restricting
/// this profile to `CORE-CAP-004`'s own tests is therefore a scoping decision,
/// not a waiver: nothing is exempted from grading, it is graded elsewhere. The
/// alternative upstream records other SDKs taking — a `sitecustomize.py` shim
/// that monkey-patches the harness into sending the header — is deliberately
/// not used here, because it would change the suite rather than the SUT.
const REQUIRED_EXTENSION_URI: &str = "urn:a2a:tck:required-extension";

fn make_agent_card(
    base_url: &str,
    grpc_url: &str,
    ws_url: Option<&str>,
    profile: Profile,
) -> AgentCard {
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
            // The TCK builds one client per advertised interface, so declaring
            // GRPC is what makes it run the whole core suite a third time over
            // the gRPC binding — and what turns the six `GRPC-*` MUST
            // requirements from SKIPPED into a real pass/fail result.
            AgentInterface {
                url: grpc_url.into(),
                protocol_binding: "GRPC".into(),
                protocol_version: a2a_protocol_types::A2A_VERSION.into(),
                tenant: None,
            },
        ]
        .into_iter()
        // §12's custom binding, advertised only when the listener is up. The
        // official suite ignores a binding name it does not know; the in-repo
        // TCK's `--binding websocket` leg discovers the socket here, the same
        // way a real client would.
        .chain(ws_url.map(|url| AgentInterface {
            url: url.into(),
            protocol_binding: "WEBSOCKET".into(),
            protocol_version: a2a_protocol_types::A2A_VERSION.into(),
            tenant: None,
        }))
        .collect(),
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
        capabilities: match profile {
            Profile::Full => AgentCapabilities::none()
                .with_streaming(true)
                .with_push_notifications(true)
                .with_extended_agent_card(true),
            // Advertising nothing is the point: it is what lets the suite ask
            // whether unsupported operations are rejected.
            Profile::Minimal => AgentCapabilities::none(),
            // Same capabilities as Full, plus the sentinel required extension.
            // Keeping the rest identical to Full means the only variable this
            // profile introduces is the extension itself.
            Profile::Extension => {
                let mut caps = AgentCapabilities::none()
                    .with_streaming(true)
                    .with_push_notifications(true)
                    .with_extended_agent_card(true);
                caps.extensions = Some(vec![a2a_protocol_types::extensions::AgentExtension {
                    uri: REQUIRED_EXTENSION_URI.into(),
                    description: Some(
                        "TCK sentinel extension for CORE-CAP-004 (required-extension \
                         negotiation). Carries no behaviour."
                            .into(),
                    ),
                    required: Some(true),
                    params: None,
                }]);
                caps
            }
            // Same capabilities as Full. The only variable this profile
            // introduces is authentication, so anything BIND-EQUIV-004
            // observes is attributable to that and not to a capability change.
            Profile::Secured => AgentCapabilities::none()
                .with_streaming(true)
                .with_push_notifications(true)
                .with_extended_agent_card(true),
        },
        provider: None,
        icon_url: None,
        documentation_url: None,
        // Declared at card level and nowhere else, which is the shape
        // BIND-EQUIV-004's structural half asserts: v1.0 gives
        // `AgentInterface` no security fields, so one declaration here binds
        // every binding equally.
        security_schemes: match profile {
            Profile::Secured => Some(
                [(
                    SECURED_PROFILE_SCHEME.to_string(),
                    a2a_protocol_types::SecurityScheme::Http(
                        a2a_protocol_types::HttpAuthSecurityScheme {
                            scheme: "bearer".into(),
                            bearer_format: None,
                            description: Some(
                                "Static bearer token, for the TCK's BIND-EQUIV-004 \
                                 enforcement probe. Not a credential scheme to copy."
                                    .into(),
                            ),
                        },
                    ),
                )]
                .into_iter()
                .collect(),
            ),
            _ => None,
        },
        security_requirements: match profile {
            Profile::Secured => Some(vec![a2a_protocol_types::SecurityRequirement {
                schemes: [(
                    SECURED_PROFILE_SCHEME.to_string(),
                    // No scopes: a bearer token that grants everything. The
                    // requirement is about presenting a credential at all.
                    a2a_protocol_types::StringList { list: Vec::new() },
                )]
                .into_iter()
                .collect(),
            }]),
            _ => None,
        },
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

    // gRPC needs its own listener: the binding speaks HTTP/2 with its own
    // framing and cannot share the JSON-RPC/REST router.
    let grpc_host = std::env::var("SUT_GRPC_HOST")
        .unwrap_or_else(|_| format!("{}:{}", addr.ip(), addr.port().saturating_sub(1)));
    let grpc_addr: SocketAddr = grpc_host.parse().expect("SUT_GRPC_HOST must be host:port");
    // Advertised WITHOUT a scheme: a gRPC target is a name-resolver string
    // (`host:port`), not a URL. `grpc.insecure_channel("http://127.0.0.1:9998")`
    // fails with "Misformatted domain name", which the TCK reports as 25
    // failing MUST requirements that look like binding defects and are not.
    let grpc_advertised =
        std::env::var("SUT_GRPC_ADVERTISE_URL").unwrap_or_else(|_| grpc_addr.to_string());

    // §12's WebSocket binding needs its own listener too — the dispatcher
    // speaks the HTTP upgrade itself rather than sharing the router below.
    // Opt-in: the official suite does not drive it, so it stays off unless a
    // run asks for it (the in-repo TCK's websocket and equivalence legs do).
    let ws_host = std::env::var("SUT_WS_HOST").ok();
    let ws_advertised = ws_host
        .as_ref()
        .map(|h| std::env::var("SUT_WS_ADVERTISE_URL").unwrap_or_else(|_| format!("ws://{h}")));

    let profile = Profile::from_env();
    let mut builder = RequestHandlerBuilder::new(TckSutExecutor).with_agent_card(make_agent_card(
        &advertised,
        &grpc_advertised,
        ws_advertised.as_deref(),
        profile,
    ));
    // One interceptor, above the dispatchers, guarding all four bindings —
    // which is precisely the property BIND-EQUIV-004's enforcement half exists
    // to verify. It reads `Authorization` from the `CallContext`, which
    // JSON-RPC, HTTP+JSON, gRPC and WebSocket all populate; a binding that
    // stopped doing so would fail the probe rather than silently serve
    // unauthenticated traffic.
    if profile == Profile::Secured {
        builder = builder.with_interceptor(
            a2a_protocol_server::auth::BearerTokenAuthInterceptor::new([SECURED_PROFILE_TOKEN]),
        );
    }
    // Extension advertises the same capabilities as Full, so it needs the same
    // supporting stores; only Minimal advertises nothing and needs none.
    if profile != Profile::Minimal {
        builder = builder
            .with_push_config_store(InMemoryPushConfigStore::new())
            // The TCK runs its webhook receiver on loopback, which the
            // sender's SSRF guard blocks by default — correct in production,
            // wrong for a conformance harness pointing at its own listener.
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            // §13.3 requires the extended-card endpoint to be authenticated,
            // and the builder refuses to serve it without an authenticating
            // interceptor. The TCK has no credentials to present, so the SUT
            // opts in explicitly rather than shipping an auth scheme the suite
            // cannot satisfy.
            .allow_unauthenticated_extended_card();
    }
    let handler = Arc::new(builder.build().expect("build SUT request handler"));

    let grpc = a2a_protocol_server::dispatch::GrpcDispatcher::new(
        Arc::clone(&handler),
        a2a_protocol_server::dispatch::GrpcConfig::default(),
    );
    match grpc.serve_with_addr(grpc_addr).await {
        Ok(bound) => println!("a2a-rust TCK SUT gRPC listening on {bound}"),
        Err(e) => {
            // Not fatal: the JSON bindings are still worth grading. But say so
            // loudly — a silent gRPC failure would show up as six SKIPPED
            // requirements that look like a coverage gap rather than a fault.
            eprintln!("WARNING: gRPC listener failed to bind {grpc_addr}: {e}");
        }
    }

    if let Some(ws_host) = ws_host {
        let ws = Arc::new(
            a2a_protocol_server::dispatch::websocket::WebSocketDispatcher::new(Arc::clone(
                &handler,
            )),
        );
        match ws.serve_with_addr(ws_host.as_str()).await {
            Ok(bound) => println!("a2a-rust TCK SUT WebSocket listening on ws://{bound}"),
            // Fatal, unlike the gRPC warning above: SUT_WS_HOST is only set by
            // a run that intends to grade the WebSocket binding, and a leg
            // whose target silently is not there reports connection errors as
            // conformance failures.
            Err(e) => panic!("SUT_WS_HOST={ws_host} was set but the listener failed to bind: {e}"),
        }
    }

    serve(addr, handler).await;
}
