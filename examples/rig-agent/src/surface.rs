// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Serving and client wiring for the surface run.
//!
//! # Why an LLM example measures a protocol surface at all
//!
//! The interesting part of this example is the executor — an LLM behind the
//! A2A protocol. The *protocol* part is the same for every agent, and that is
//! precisely why it is worth measuring here: an example whose executor is
//! exotic is exactly where a transport quietly goes unserved and nobody
//! notices, because attention is on the model.
//!
//! Before 2026-08-11 this example served one binding of four and drove none of
//! the eleven methods itself; it printed a URL and waited for `Ctrl+C`.
//!
//! The model is not required for any of that. [`probe_model`] asks separately
//! whether an LLM was reachable and the run says so either way, so "the
//! protocol works" is never quietly upgraded to "the model works".

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_example_harness::Binding;
use a2a_example_harness::{interfaces, Endpoints};
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::handler::RequestHandler;
use a2a_protocol_server::push::{HttpPushSender, InMemoryPushConfigStore};
use a2a_protocol_types::agent_card::AgentCapabilities;

use crate::{make_agent_card, RigAgentExecutor};

type BoxErr = Box<dyn std::error::Error>;

async fn bind() -> Result<(tokio::net::TcpListener, SocketAddr), BoxErr> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let a = l.local_addr()?;
    Ok((l, a))
}

/// Starts the agent on all four bindings, with the mechanical fallback on.
pub async fn start<M>(
    model: &str,
    agent: impl Fn() -> Result<rig_core::agent::Agent<M>, String>,
) -> Result<Endpoints, BoxErr>
where
    M: rig_core::completion::CompletionModel + 'static,
{
    let (http_l, http_a) = bind().await?;
    let (grpc_l, grpc_a) = bind().await?;
    let (ws_probe, ws_a) = bind().await?;
    drop(ws_probe); // the WebSocket dispatcher binds its own listener

    let url = format!("http://{http_a}");
    let ep = Endpoints {
        jsonrpc: url.clone(),
        rest: url.clone(),
        grpc: grpc_a.to_string(),
        websocket: format!("ws://{ws_a}"),
    };

    let mut card = make_agent_card(&url, model);
    card.supported_interfaces = interfaces(&ep);

    let handler: Arc<RequestHandler> = Arc::new(
        RequestHandlerBuilder::new(RigAgentExecutor {
            agent: agent()?,
            fallback_on_error: true,
        })
        .with_agent_card(card)
        .with_push_config_store(InMemoryPushConfigStore::new())
        .with_push_sender(HttpPushSender::new().allow_private_urls())
        .allow_unauthenticated_extended_card()
        .build()?,
    );

    serve_combined(http_l, Arc::clone(&handler));
    {
        use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};
        GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default())
            .serve_with_listener(grpc_l)?;
    }
    {
        let ws = Arc::new(
            a2a_protocol_server::dispatch::websocket::WebSocketDispatcher::new(Arc::clone(
                &handler,
            )),
        );
        ws.serve_with_addr(ws_a.to_string().as_str()).await?;
    }

    Ok(ep)
}

/// JSON-RPC and REST on one socket, routed by request shape.
fn serve_combined(listener: tokio::net::TcpListener, handler: Arc<RequestHandler>) {
    let jsonrpc = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));
    let rest = Arc::new(RestDispatcher::new(handler));
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
}

/// An agent advertising no optional capabilities, for the counter-tests.
pub async fn start_restricted<M>(
    model: &str,
    agent: impl Fn() -> Result<rig_core::agent::Agent<M>, String>,
) -> Result<String, BoxErr>
where
    M: rig_core::completion::CompletionModel + 'static,
{
    let (listener, addr) = bind().await?;
    let url = format!("http://{addr}");
    let mut card = make_agent_card(&url, model);
    card.name = "Restricted Rig Agent".into();
    card.capabilities = AgentCapabilities::none();

    let handler: Arc<RequestHandler> = Arc::new(
        RequestHandlerBuilder::new(RigAgentExecutor {
            agent: agent()?,
            fallback_on_error: true,
        })
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

/// A webhook sink, so push configs point at something that answers.
pub async fn start_webhook() -> Result<String, BoxErr> {
    let (listener, addr) = bind().await?;
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

/// Asks whether a real model answered, by sending one request and looking for
/// the fallback label.
///
/// Deliberately not inferred from "the sweep passed": the fallback keeps the
/// protocol working with no model at all, so a green sweep says nothing about
/// the LLM. This is the only thing in the run that can tell them apart.
pub async fn probe_model(jsonrpc_url: &str) -> bool {
    let Ok(client) = ClientBuilder::new(jsonrpc_url).build() else {
        return false;
    };
    let params = a2a_example_harness::make_send_params("ping");
    match client.send_message(params).await {
        Ok(a2a_protocol_types::responses::SendMessageResponse::Task(task)) => task
            .artifacts
            .as_deref()
            .and_then(<[a2a_protocol_types::artifact::Artifact]>::first)
            .map(|a| {
                a.parts.iter().any(|p| match &p.content {
                    a2a_protocol_types::message::PartContent::Text(t) => {
                        !t.contains("no model reachable")
                    }
                    _ => false,
                })
            })
            .unwrap_or(false),
        _ => false,
    }
}

/// Builds a client speaking `binding`.
pub async fn client_for(binding: Binding, ep: &Endpoints) -> Result<A2aClient, String> {
    match binding {
        Binding::JsonRpc => ClientBuilder::new(&ep.jsonrpc)
            .build()
            .map_err(|e| e.to_string()),
        Binding::HttpJson => ClientBuilder::new(&ep.rest)
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
            let t =
                a2a_protocol_client::transport::WebSocketTransport::connect(ep.websocket.clone())
                    .await
                    .map_err(|e| format!("WebSocket connect: {e}"))?;
            ClientBuilder::new(&ep.websocket)
                .with_custom_transport(t)
                .without_tls()
                .build()
                .map_err(|e| e.to_string())
        }
    }
}
