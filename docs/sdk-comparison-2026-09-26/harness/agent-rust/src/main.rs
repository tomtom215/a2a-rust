// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Benchmark agent built on a2a-rust (a2a-protocol-sdk 0.14.0 from crates.io).
//!
//! BEHAVIOUR CONTRACT (identical in agent-rs):
//!   * text starting "wait:" -> WORKING, then stay open until canceled.
//!   * AGENT_MODE=echo       -> WORKING, one artifact "Echo: <text>" (lastChunk), COMPLETED.
//!   * AGENT_MODE=llm        -> WORKING, one artifact streamed from the model: first chunk
//!                              append=false, later chunks append=true, final chunk
//!                              lastChunk=true; then COMPLETED. Model error -> FAILED.
//!   Card: streaming, pushNotifications, extendedAgentCard all true;
//!   JSON-RPC, HTTP+JSON and gRPC interfaces advertised.

#[path = "../../common/llm.rs"]
mod llm;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};
use a2a_protocol_sdk::server::push::{HttpPushSender, InMemoryPushConfigStore};
use a2a_protocol_sdk::types::agent_card::AgentCapabilities;
use a2a_protocol_sdk::types::failure::FailureClass;
use a2a_protocol_sdk::server::request_context::RequestContext;
use a2a_protocol_sdk::server::streaming::EventQueueWriter;
use std::sync::Arc;

struct Bench {
    llm: bool,
}

// `agent_executor!` cannot be used: its body has no access to `self`, so the
// executor's mode flag is unreachable. Implemented by hand instead.
impl AgentExecutor for Bench {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            let text = ctx.message.text().unwrap_or("").to_string();

            if text.starts_with("wait:") {
                ctx.cancellation_token.cancelled().await;
                return Ok(());
            }

            if !self.llm {
                emit.artifact("answer", vec![Part::text(format!("Echo: {text}"))], None, Some(true))
                    .await?;
                return emit.status(TaskState::Completed).await;
            }

            let mut rx = llm::stream_completion(text);
            let mut pending: Option<String> = None;
            let mut first = true;
            while let Some(item) = rx.recv().await {
                match item {
                    Ok(delta) => {
                        if let Some(prev) = pending.replace(delta) {
                            emit.artifact("answer", vec![Part::text(prev)], Some(!first), Some(false))
                                .await?;
                            first = false;
                        }
                    }
                    Err(e) => return emit.fail(FailureClass::Transient, e).await,
                }
            }
            let last = pending.unwrap_or_default();
            emit.artifact("answer", vec![Part::text(last)], Some(!first), Some(true))
                .await?;
            emit.status(TaskState::Completed).await
        })
    }
}

fn env_port(name: &str, default: u16) -> u16 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let llm = std::env::var("AGENT_MODE").as_deref() == Ok("llm");
    let jp = env_port("JSONRPC_PORT", 7101);
    let rp = env_port("REST_PORT", 7102);
    let gp = env_port("GRPC_PORT", 7103);

    let mut caps = AgentCapabilities::default();
    caps.streaming = Some(true);
    caps.push_notifications = Some(true);
    caps.extended_agent_card = Some(true);
    let card = AgentCard::new(
        "bench-agent",
        "1.0.0",
        AgentInterface::jsonrpc(format!("http://127.0.0.1:{jp}")),
    )
    .with_description("Benchmark agent (a2a-rust)")
    .with_input_modes(["text/plain"])
    .with_output_modes(["text/plain"])
    .with_interface(AgentInterface::rest(format!("http://127.0.0.1:{rp}")))
    .with_interface(AgentInterface::grpc(format!("http://127.0.0.1:{gp}")))
    .with_capabilities(caps);

    let handler = Arc::new(
        RequestHandlerBuilder::new(Bench { llm })
            .with_agent_card(card)
            .with_push_config_store(InMemoryPushConfigStore::new())
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .allow_unauthenticated_extended_card()
            .build()
            .expect("handler"),
    );

    let grpc = tokio::net::TcpListener::bind(("127.0.0.1", gp)).await?;
    if std::env::var("NODELAY").is_ok() {
        // Same wiring as agent-rs's variant: SDK service, tonic server, NODELAY incoming.
        use futures::StreamExt;
        let svc = GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default()).into_service();
        let inc = tokio_stream::wrappers::TcpListenerStream::new(grpc)
            .map(|s| { if let Ok(t) = &s { let _ = t.set_nodelay(true); } s });
        tokio::spawn(tonic::transport::Server::builder().add_service(svc).serve_with_incoming(inc));
    } else {
        GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default())
            .serve_with_listener(grpc)
            .expect("grpc");
    }
    let rest = tokio::spawn(serve(("127.0.0.1", rp), RestDispatcher::new(Arc::clone(&handler))));
    eprintln!("agent-rust ready jsonrpc={jp} rest={rp} grpc={gp} llm={llm}");
    tokio::select! {
        r = serve(("127.0.0.1", jp), JsonRpcDispatcher::new(handler)) => r,
        r = rest => r.expect("join"),
    }
}
