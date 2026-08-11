// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A fifth agent that serves all four bindings, so the method × binding matrix
//! can be measured.
//!
//! # Why a separate agent rather than reusing the team
//!
//! The four team agents are deliberately split by binding — CodeAnalyzer on
//! JSON-RPC, BuildMonitor on REST, and so on — because the team exists to
//! demonstrate agents talking to each other over different transports. That
//! split is the point, and it also means no single one of them can answer the
//! question "does every method work on every binding?".
//!
//! The 100 E2E tests measure *features*. They do not measure the protocol
//! surface: before this module, `SubscribeToTask` was driven on two bindings,
//! `GetExtendedAgentCard` on one, and nothing anywhere checked whether that
//! was deliberate. A feature table cannot answer a coverage question it has no
//! rows for.
//!
//! So this agent exists purely to be swept. It shares the scorer with
//! `echo-agent` and `incident-response` (`a2a-example-harness`), which is what
//! makes the three examples' claims comparable rather than three different
//! definitions of "complete".

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::handler::RequestHandler;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

use a2a_example_harness::Binding;

use crate::infrastructure::{bind_listener, serve_jsonrpc, serve_rest};

/// Message-text prefix that makes the executor pause mid-task.
///
/// `SubscribeToTask` needs a task that is still running — the server refuses to
/// re-attach to a terminal one, correctly — so without a slow turn the success
/// path is unreachable and only the refusal is ever observed.
pub const SLOW_PREFIX: &str = "slow:";

/// Where the surface agent is listening, per binding.
pub struct SurfaceEndpoints {
    /// JSON-RPC over HTTP (§9).
    pub jsonrpc: String,
    /// HTTP+JSON (§11).
    pub rest: String,
    /// gRPC (§10), as `host:port`.
    pub grpc: String,
    /// WebSocket (§12 custom), as a `ws://` URL.
    pub websocket: String,
}

impl SurfaceEndpoints {
    /// The URL a client for `binding` should target.
    #[must_use]
    pub fn for_binding(&self, binding: Binding) -> &str {
        match binding {
            Binding::JsonRpc => &self.jsonrpc,
            Binding::HttpJson => &self.rest,
            Binding::Grpc => &self.grpc,
            Binding::WebSocket => &self.websocket,
        }
    }
}

/// Echoes its input, pausing first when asked.
struct SurfaceExecutor;

impl AgentExecutor for SurfaceExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            let text = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    a2a_protocol_types::message::PartContent::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .unwrap_or("<no text>");

            if text.starts_with(SLOW_PREFIX) {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }

            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("surface", vec![Part::text(&format!("echo: {text}"))]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

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

fn card(ep: &SurfaceEndpoints, name: &str, capabilities: AgentCapabilities) -> AgentCard {
    let iface = |url: &str, binding: &str| AgentInterface {
        url: url.to_owned(),
        protocol_binding: binding.to_owned(),
        protocol_version: a2a_protocol_types::A2A_VERSION.into(),
        tenant: None,
    };
    AgentCard {
        url: None,
        name: name.into(),
        description: "Serves every binding so the surface matrix can be measured".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![
            iface(&ep.jsonrpc, "JSONRPC"),
            iface(&ep.rest, "HTTP+JSON"),
            iface(&ep.grpc, "GRPC"),
            iface(&ep.websocket, "WEBSOCKET"),
        ],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "surface".into(),
            name: "Surface".into(),
            description: "Echoes input; pauses when asked".into(),
            tags: vec!["surface".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities,
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// Starts the surface agent on all four bindings.
pub async fn start() -> SurfaceEndpoints {
    let (jsonrpc_l, jsonrpc_a) = bind_listener().await;
    let (rest_l, rest_a) = bind_listener().await;
    let (grpc_l, grpc_a) = bind_listener().await;
    let (ws_probe, ws_a) = bind_listener().await;
    drop(ws_probe); // the WebSocket dispatcher binds its own listener

    let ep = SurfaceEndpoints {
        jsonrpc: format!("http://{jsonrpc_a}"),
        rest: format!("http://{rest_a}"),
        grpc: grpc_a.to_string(),
        websocket: format!("ws://{ws_a}"),
    };

    let handler: Arc<RequestHandler> = Arc::new(
        RequestHandlerBuilder::new(SurfaceExecutor)
            .with_agent_card(card(
                &ep,
                "Surface Agent",
                AgentCapabilities::none()
                    .with_streaming(true)
                    .with_push_notifications(true)
                    .with_extended_agent_card(true),
            ))
            .with_push_config_store(a2a_protocol_server::push::InMemoryPushConfigStore::new())
            .with_push_sender(a2a_protocol_server::push::HttpPushSender::new().allow_private_urls())
            // §13.3 wants the extended card authenticated; this agent ships no
            // authenticator, so the opt-in is explicit. The refusal path is
            // covered by the counter-tests against the restricted agent below.
            .allow_unauthenticated_extended_card()
            .build()
            .expect("build surface handler"),
    );

    serve_jsonrpc(jsonrpc_l, Arc::clone(&handler));
    serve_rest(rest_l, Arc::clone(&handler));
    {
        use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};
        GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default())
            .serve_with_listener(grpc_l)
            .expect("start surface gRPC");
    }
    {
        let ws = Arc::new(
            a2a_protocol_server::dispatch::websocket::WebSocketDispatcher::new(Arc::clone(
                &handler,
            )),
        );
        ws.serve_with_addr(ws_a.to_string().as_str())
            .await
            .expect("start surface WebSocket");
    }

    ep
}

/// Starts an agent advertising no optional capabilities, for the
/// counter-tests. One agent cannot both support and refuse a capability.
pub async fn start_restricted() -> String {
    let (listener, addr) = bind_listener().await;
    let url = format!("http://{addr}");
    let ep = SurfaceEndpoints {
        jsonrpc: url.clone(),
        rest: url.clone(),
        grpc: addr.to_string(),
        websocket: format!("ws://{addr}"),
    };
    let mut c = card(&ep, "Restricted Surface Agent", AgentCapabilities::none());
    c.supported_interfaces.truncate(1);

    let handler: Arc<RequestHandler> = Arc::new(
        RequestHandlerBuilder::new(SurfaceExecutor)
            .with_agent_card(c)
            .build()
            .expect("build restricted handler"),
    );
    serve_jsonrpc(listener, handler);
    url
}

/// Builds a client speaking `binding` against the surface agent.
pub async fn client_for(
    binding: Binding,
    ep: &SurfaceEndpoints,
) -> Result<a2a_protocol_client::A2aClient, String> {
    use a2a_protocol_client::ClientBuilder;
    let url = ep.for_binding(binding);
    match binding {
        Binding::JsonRpc => ClientBuilder::new(url).build().map_err(|e| e.to_string()),
        Binding::HttpJson => ClientBuilder::new(url)
            .with_protocol_binding("HTTP+JSON")
            .build()
            .map_err(|e| e.to_string()),
        Binding::Grpc => {
            let full = format!("http://{url}");
            let t = a2a_protocol_client::transport::grpc::GrpcTransport::connect(&full)
                .await
                .map_err(|e| format!("gRPC connect: {e}"))?;
            ClientBuilder::new(&full)
                .with_custom_transport(t)
                .without_tls()
                .build()
                .map_err(|e| e.to_string())
        }
        Binding::WebSocket => {
            let t = a2a_protocol_client::transport::WebSocketTransport::connect(url.to_owned())
                .await
                .map_err(|e| format!("WebSocket connect: {e}"))?;
            ClientBuilder::new(url)
                .with_custom_transport(t)
                .without_tls()
                .build()
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{card, SurfaceEndpoints};
    use a2a_example_harness::Binding;
    use a2a_protocol_types::agent_card::AgentCapabilities;

    fn ep() -> SurfaceEndpoints {
        SurfaceEndpoints {
            jsonrpc: "http://127.0.0.1:1".into(),
            rest: "http://127.0.0.1:2".into(),
            grpc: "127.0.0.1:3".into(),
            websocket: "ws://127.0.0.1:4".into(),
        }
    }

    /// A column the matrix scores but the card does not advertise is
    /// undiscoverable: §5 makes the card the only way a client finds an
    /// endpoint, so the matrix would report cells no real client could reach.
    #[test]
    fn the_card_advertises_every_binding_the_matrix_scores() {
        let c = card(&ep(), "t", AgentCapabilities::none());
        for b in Binding::ALL {
            assert!(
                c.supported_interfaces
                    .iter()
                    .any(|i| i.protocol_binding == b.label()),
                "matrix scores {} but the card does not advertise it",
                b.label()
            );
        }
    }

    /// `for_binding` must be total and must not alias two bindings onto one
    /// endpoint — that would silently sweep the same transport twice and
    /// report the other as covered.
    #[test]
    fn every_binding_maps_to_a_distinct_endpoint() {
        let e = ep();
        let urls: Vec<&str> = Binding::ALL.iter().map(|b| e.for_binding(*b)).collect();
        let mut sorted = urls.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            urls.len(),
            "two bindings share an endpoint: {urls:?}"
        );
    }
}
