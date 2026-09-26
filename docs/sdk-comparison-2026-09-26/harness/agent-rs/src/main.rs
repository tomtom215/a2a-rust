// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Benchmark agent built on a2a-rs (a2a-lf 0.3.1 / a2a-server-lf 0.4.4 /
//! a2a-grpc 0.3.7 from crates.io).
//!
//! BEHAVIOUR CONTRACT (identical in agent-rust):
//!   * text starting "wait:" -> WORKING, then stay open until canceled.
//!   * AGENT_MODE=echo       -> WORKING, one artifact "Echo: <text>" (lastChunk), COMPLETED.
//!   * AGENT_MODE=llm        -> WORKING, one artifact streamed from the model: first chunk
//!                              append=false, later chunks append=true, final chunk
//!                              lastChunk=true; then COMPLETED. Model error -> FAILED.
//!   Card: streaming, pushNotifications, extendedAgentCard all true;
//!   JSON-RPC, HTTP+JSON and gRPC interfaces advertised.

#[path = "../../common/llm.rs"]
mod llm;

use a2a::event::StreamResponse;
use a2a::*;
use a2a_grpc::GrpcHandler;
use a2a_pb::proto::a2a_service_server::A2aServiceServer;
use a2a_server::*;
use futures::stream::BoxStream;
use std::future::IntoFuture;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};

struct Bench {
    llm: bool,
}

fn status(task_id: &TaskId, context_id: &str, state: TaskState, msg: Option<Message>) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: task_id.clone(),
        context_id: context_id.to_string(),
        status: TaskStatus { state, message: msg, timestamp: Some(chrono::Utc::now()) },
        metadata: None,
    })
}

fn artifact(task_id: &TaskId, context_id: &str, text: String, append: Option<bool>, last: Option<bool>) -> StreamResponse {
    StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
        task_id: task_id.clone(),
        context_id: context_id.to_string(),
        artifact: Artifact {
            artifact_id: "answer".into(),
            name: None,
            description: None,
            parts: vec![Part::text(text)],
            metadata: None,
            extensions: None,
        },
        append,
        last_chunk: last,
        metadata: None,
    })
}

impl AgentExecutor for Bench {
    fn execute(&self, ctx: ExecutorContext) -> BoxStream<'static, Result<StreamResponse, A2AError>> {
        let llm = self.llm;
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let tid = ctx.task_id.clone();
            let cid = ctx.context_id.clone();
            let text = ctx
                .message
                .as_ref()
                .and_then(|m| m.parts.iter().find_map(|p| match &p.content {
                    PartContent::Text(t) => Some(t.clone()),
                    _ => None,
                }))
                .unwrap_or_default();
            if tx.send(Ok(status(&tid, &cid, TaskState::Working, None))).await.is_err() {
                return;
            }
            if text.starts_with("wait:") {
                // Stay open until the consumer drops the stream (cancel).
                tx.closed().await;
                return;
            }
            if !llm {
                let _ = tx.send(Ok(artifact(&tid, &cid, format!("Echo: {text}"), None, Some(true)))).await;
                let _ = tx.send(Ok(status(&tid, &cid, TaskState::Completed, None))).await;
                return;
            }
            let mut model = llm::stream_completion(text);
            let mut pending: Option<String> = None;
            let mut first = true;
            while let Some(item) = model.recv().await {
                match item {
                    Ok(delta) => {
                        if let Some(prev) = pending.replace(delta) {
                            if tx.send(Ok(artifact(&tid, &cid, prev, Some(!first), Some(false)))).await.is_err() {
                                return;
                            }
                            first = false;
                        }
                    }
                    Err(e) => {
                        let msg = Message::new(Role::Agent, vec![Part::text(e)]);
                        let _ = tx.send(Ok(status(&tid, &cid, TaskState::Failed, Some(msg)))).await;
                        return;
                    }
                }
            }
            let last = pending.unwrap_or_default();
            let _ = tx.send(Ok(artifact(&tid, &cid, last, Some(!first), Some(true)))).await;
            let _ = tx.send(Ok(status(&tid, &cid, TaskState::Completed, None))).await;
        });
        Box::pin(ReceiverStream::new(rx))
    }

    fn cancel(&self, ctx: ExecutorContext) -> BoxStream<'static, Result<StreamResponse, A2AError>> {
        let ev = status(&ctx.task_id, &ctx.context_id, TaskState::Canceled, None);
        Box::pin(futures::stream::once(async move { Ok(ev) }))
    }
}

fn env_port(name: &str, default: u16) -> u16 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::main]
async fn main() {
    let llm = std::env::var("AGENT_MODE").as_deref() == Ok("llm");
    let hp = env_port("HTTP_PORT", 7201);
    let gp = env_port("GRPC_PORT", 7203);

    let caps = AgentCapabilities {
        streaming: Some(true),
        push_notifications: Some(true),
        extensions: None,
        extended_agent_card: Some(true),
    };
    let card = AgentCard {
        name: "bench-agent".into(),
        description: "Benchmark agent (a2a-rs)".into(),
        version: "1.0.0".into(),
        provider: None,
        capabilities: caps.clone(),
        skills: vec![],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        supported_interfaces: vec![
            AgentInterface::new(format!("http://127.0.0.1:{hp}/jsonrpc"), TRANSPORT_PROTOCOL_JSONRPC),
            AgentInterface::new(format!("http://127.0.0.1:{hp}/rest"), TRANSPORT_PROTOCOL_HTTP_JSON),
            AgentInterface::new(format!("http://127.0.0.1:{gp}"), TRANSPORT_PROTOCOL_GRPC),
        ],
        security_schemes: None,
        security_requirements: None,
        documentation_url: None,
        icon_url: None,
        signatures: None,
    };
    let sender = HttpPushSender::new(Some(HttpPushSenderConfig {
        validate_urls: false, // local webhook in the test harness
        ..Default::default()
    }));
    let handler = Arc::new(
        DefaultRequestHandler::new(Bench { llm }, InMemoryTaskStore::new())
            .with_push_notifications(InMemoryPushConfigStore::new(), sender)
            .with_extended_agent_card(card.clone())
            .with_capabilities(caps),
    );
    let app = axum::Router::new()
        .nest("/jsonrpc", a2a_server::jsonrpc::jsonrpc_router(handler.clone()))
        .nest("/rest", a2a_server::rest::rest_router(handler.clone()))
        .merge(a2a_server::agent_card::agent_card_router(Arc::new(StaticAgentCard::new(card))));
    let grpc = A2aServiceServer::new(GrpcHandler::new(handler));

    let http_l = tokio::net::TcpListener::bind(("127.0.0.1", hp)).await.unwrap();
    let grpc_l = tokio::net::TcpListener::bind(("127.0.0.1", gp)).await.unwrap();
    eprintln!("agent-rs ready http={hp} grpc={gp} llm={llm}");
    tokio::select! {
        r = async { if std::env::var("NODELAY").is_ok() { use axum::serve::ListenerExt; axum::serve(http_l.tap_io(|t| { let _ = t.set_nodelay(true); }), app).into_future().await } else { axum::serve(http_l, app).into_future().await } } => { r.unwrap() }
        r = async {
              use futures::StreamExt;
              let nd = std::env::var("NODELAY").is_ok();
              let inc = TcpListenerStream::new(grpc_l).map(move |s| { if let Ok(t) = &s { if nd { let _ = t.set_nodelay(true); } } s });
              tonic::transport::Server::builder().add_service(grpc).serve_with_incoming(inc).await
          } => { r.unwrap() }
    }
}
