// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Benchmark agent on a2a-rust (a2a-protocol-sdk 0.14.1). Behaviour contract,
//! identical in both harness agents:
//!   text "llm:<prompt>" -> Working, N artifact chunks (one per model delta,
//!                          append=true after the first), last_chunk, Completed
//!   anything else       -> Working, one artifact "Echo: <text>", Completed
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_sdk::prelude::*;

#[cfg(feature = "llm")]
#[path = "../../shared/llm.rs"]
mod llm;

struct BenchExecutor {
    #[cfg(feature = "llm")]
    http: reqwest::Client,
}

impl AgentExecutor for BenchExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            let text = ctx.message.text().unwrap_or("").to_string();
            emit.status(TaskState::Working).await?;
            #[cfg(feature = "llm")]
            if let Some(prompt) = text.strip_prefix("llm:") {
                let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
                let http = self.http.clone();
                let prompt = prompt.to_string();
                tokio::spawn(async move {
                    let _ = llm::stream_completion(&http, &prompt, 64, tx).await;
                });
                let mut first = true;
                while let Some(delta) = rx.recv().await {
                    emit.artifact("reply", vec![Part::text(delta)], Some(!first), Some(false)).await?;
                    first = false;
                }
                emit.artifact("reply", vec![Part::text(String::new())], Some(true), Some(true)).await?;
                emit.status(TaskState::Completed).await?;
                return Ok(());
            }
            if text.starts_with("wait:") {
                ctx.cancellation_token.cancelled().await;
                return Ok(());
            }
            emit.artifact("reply", vec![Part::text(format!("Echo: {text}"))], Some(false), Some(true)).await?;
            emit.status(TaskState::Completed).await?;
            Ok(())
        })
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3002);
    let rest_port = port + 1000;
    let url = format!("http://127.0.0.1:{port}");
    let card = AgentCard::new("bench-a2a-rust", "0.0.0", AgentInterface::jsonrpc(&url))
        .with_description("benchmark agent");
    let mut card = card;
    card.capabilities.streaming = Some(true);
    card.supported_interfaces.push(AgentInterface::new(format!("http://127.0.0.1:{rest_port}"), "HTTP+JSON"));
    let exec = BenchExecutor {
        #[cfg(feature = "llm")]
        http: reqwest::Client::new(),
    };
    let mut builder = RequestHandlerBuilder::new(exec).with_agent_card(card);
    if std::env::var("NO_TTL_SWEEP").as_deref() == Ok("1") {
        // Attribution probe only: the TTL pass never runs; the capacity cap still holds.
        eprintln!("TTL sweep disabled (eviction_interval = 0)");
        let mut cfg = a2a_protocol_sdk::server::TaskStoreConfig::default();
        cfg.eviction_interval = 0;
        builder = builder.with_task_store_config(cfg);
    }
    if std::env::var("TENANT_STORE").as_deref() == Ok("1") {
        eprintln!("using TenantAwareInMemoryTaskStore + TenantAwareInMemoryPushConfigStore");
        builder = builder
            .with_task_store(a2a_protocol_sdk::server::TenantAwareInMemoryTaskStore::new())
            .with_push_config_store(a2a_protocol_sdk::server::TenantAwareInMemoryPushConfigStore::new());
    }
    #[cfg(feature = "sqlite")]
    if let Ok(url) = std::env::var("SQLITE_URL") {
        eprintln!("using SqliteTaskStore at {url}");
        builder = builder.with_task_store(
            a2a_protocol_sdk::server::SqliteTaskStore::new(&url).await.expect("open sqlite"),
        );
    }
    if let Ok(tok) = std::env::var("AUTH_TOKEN") {
        eprintln!("requiring bearer token");
        builder = builder.with_interceptor(BearerTokenAuthInterceptor::new([tok]));
    }
    let handler = Arc::new(builder.build().expect("static config"));
    eprintln!("agent-a2a-rust listening on {url} (REST on {rest_port})");
    let rest = tokio::spawn(serve(("127.0.0.1", rest_port), RestDispatcher::new(handler.clone())));
    let rpc = serve(("127.0.0.1", port), JsonRpcDispatcher::new(handler));
    tokio::select! { r = rpc => r, r = rest => r.expect("join") }
}
