// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Binding agents to sockets, and the clients that dial them.
//!
//! Every agent in this example is served on all four A2A bindings from one
//! call, so nothing in the demo can accidentally exercise a single transport
//! and look complete. The card each agent publishes is built here too, because
//! the card is what tells a client which of those four to use.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use a2a_example_harness::Binding;
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::handler::RequestHandler;
use a2a_protocol_server::push::{HttpPushSender, InMemoryPushConfigStore};
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};

use crate::agents::{LogSearchExecutor, RunbookExecutor, TriageExecutor};
use crate::{incident_model, LOGS_PORT, RUNBOOK_PORT, TRIAGE_PORT};

// ── Server scaffolding ───────────────────────────────────────────────────────

pub(crate) fn make_card(
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
pub(crate) async fn start_agent(
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

pub(crate) async fn start_logs_agent() -> Result<AgentEndpoints, Box<dyn std::error::Error>> {
    start_agent(
        LOGS_PORT,
        "Log Search Agent",
        "Deterministic log search over the service log (no LLM)",
        "log-search",
        LogSearchExecutor,
    )
    .await
}

pub(crate) async fn start_runbook_agent() -> Result<AgentEndpoints, Box<dyn std::error::Error>> {
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

pub(crate) async fn start_triage_agent(
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

/// A webhook sink so push configs point at something that answers.
///
/// A config accepted against a dead URL proves storage, not delivery.
pub(crate) async fn start_webhook_sink() -> Result<String, Box<dyn std::error::Error>> {
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
pub(crate) async fn build_client(
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
pub(crate) async fn start_restricted_agent() -> Result<String, Box<dyn std::error::Error>> {
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
