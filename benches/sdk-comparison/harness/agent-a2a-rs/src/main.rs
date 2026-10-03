// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Benchmark agent on a2a-rs (a2a-server-lf 0.5.1). Behaviour contract, identical
//! in both harness agents:
//!   text "llm:<prompt>" -> Working, N artifact chunks (one per model delta,
//!                          append=true after the first), last_chunk, Completed
//!   anything else       -> Working, one artifact "Echo: <text>", Completed
use std::sync::Arc;

use a2a::*;
use a2a_server::*;
use futures::stream::{self, BoxStream, StreamExt};

#[cfg(feature = "llm")]
#[path = "../../shared/llm.rs"]
mod llm;

#[cfg(all(feature = "dhat-heap", feature = "alloc-count"))]
compile_error!("`dhat-heap` and `alloc-count` each install a global allocator; enable one");

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static DHAT: dhat::Alloc = dhat::Alloc;

/// Runs `fut` to completion, or for `RUN_SECS` seconds when that is set, so a
/// profiler that reports on exit (dhat, heaptrack) gets a clean exit.
async fn run_bounded<F: std::future::Future>(fut: F) {
    match std::env::var("RUN_SECS").ok().and_then(|s| s.parse::<u64>().ok()) {
        Some(secs) => { let _ = tokio::time::timeout(std::time::Duration::from_secs(secs), fut).await; }
        None => { fut.await; }
    }
}

#[cfg(feature = "alloc-count")]
#[path = "../../shared/alloc_count.rs"]
mod alloc_count;
#[cfg(feature = "alloc-count")]
#[global_allocator]
static GLOBAL: alloc_count::Counting = alloc_count::Counting;

struct BenchExecutor {
    #[cfg(feature = "llm")]
    http: reqwest::Client,
}

fn status(task_id: &TaskId, ctx: &str, state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: task_id.clone(),
        context_id: ctx.to_string(),
        status: TaskStatus { state, message: None, timestamp: Some(chrono::Utc::now()) },
        metadata: None,
    })
}

fn artifact(task_id: &TaskId, ctx: &str, id: &ArtifactId, text: String, append: bool, last: bool) -> StreamResponse {
    StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
        task_id: task_id.clone(),
        context_id: ctx.to_string(),
        artifact: Artifact {
            artifact_id: id.clone(),
            name: Some("reply".into()),
            description: None,
            parts: vec![Part::text(text)],
            metadata: None,
            extensions: None,
        },
        append: Some(append),
        last_chunk: Some(last),
        metadata: None,
    })
}

impl AgentExecutor for BenchExecutor {
    fn execute(&self, ctx: ExecutorContext) -> BoxStream<'static, Result<StreamResponse, A2AError>> {
        let text = ctx.message.as_ref().and_then(|m| m.text()).unwrap_or("").to_string();
        let (tid, cid) = ctx.task_info();
        let aid = new_artifact_id();
        #[cfg(feature = "llm")]
        if let Some(prompt) = text.strip_prefix("llm:") {
            let (tx, rx) = tokio::sync::mpsc::channel::<String>(64);
            let http = self.http.clone();
            let prompt = prompt.to_string();
            tokio::spawn(async move {
                let _ = llm::stream_completion(&http, &prompt, 64, tx).await;
            });
            let head = stream::iter([Ok(status(&tid, &cid, TaskState::Working))]);
            let (t2, c2, a2) = (tid.clone(), cid.clone(), aid.clone());
            let mut first = true;
            let body = tokio_stream::wrappers::ReceiverStream::new(rx).map(move |delta| {
                let ev = artifact(&t2, &c2, &a2, delta, !first, false);
                first = false;
                Ok(ev)
            });
            let tail = stream::iter([
                Ok(artifact(&tid, &cid, &aid, String::new(), true, true)),
                Ok(status(&tid, &cid, TaskState::Completed)),
            ]);
            return Box::pin(head.chain(body).chain(tail));
        }
        if text.starts_with("wait:") {
            return Box::pin(stream::iter([Ok(status(&tid, &cid, TaskState::Working))]).chain(stream::pending()));
        }
        Box::pin(stream::iter([
            Ok(status(&tid, &cid, TaskState::Working)),
            Ok(artifact(&tid, &cid, &aid, format!("Echo: {text}"), false, true)),
            Ok(status(&tid, &cid, TaskState::Completed)),
        ]))
    }

    fn cancel(&self, ctx: ExecutorContext) -> BoxStream<'static, Result<StreamResponse, A2AError>> {
        let (tid, cid) = ctx.task_info();
        Box::pin(stream::iter([Ok(status(&tid, &cid, TaskState::Canceled))]))
    }
}

#[tokio::main]
async fn main() {
    #[cfg(feature = "dhat-heap")]
    let _dhat = dhat::Profiler::new_heap();
    #[cfg(feature = "alloc-count")]
    alloc_count::spawn_reporter();
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3001);
    let url = format!("http://127.0.0.1:{port}");
    let exec = BenchExecutor {
        #[cfg(feature = "llm")]
        http: reqwest::Client::new(),
    };
    let handler = Arc::new(DefaultRequestHandler::new(exec, InMemoryTaskStore::new()));
    let card = AgentCard {
        name: "bench-a2a-rs".into(),
        description: "benchmark agent".into(),
        version: "0.0.0".into(),
        provider: None,
        capabilities: AgentCapabilities {
            streaming: Some(true),
            push_notifications: Some(false),
            extensions: None,
            extended_agent_card: None,
        },
        skills: vec![AgentSkill {
            id: "echo".into(),
            name: "Echo".into(),
            description: "echo / llm".into(),
            tags: vec!["bench".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        supported_interfaces: vec![
            AgentInterface::new(format!("{url}/"), TRANSPORT_PROTOCOL_JSONRPC),
            AgentInterface::new(format!("{url}/rest"), TRANSPORT_PROTOCOL_HTTP_JSON),
        ],
        documentation_url: None,
        icon_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    };
    let app = axum::Router::new()
        .nest("/rest", a2a_server::rest::rest_router(handler.clone()))
        .merge(a2a_server::jsonrpc::jsonrpc_router(handler.clone()))
        .merge(a2a_server::agent_card::agent_card_router(Arc::new(StaticAgentCard::new(card))));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    eprintln!("agent-a2a-rs listening on {url}");
    if std::env::var("NODELAY").as_deref() == Ok("1") {
        eprintln!("TCP_NODELAY on accepted sockets");
        use axum::serve::ListenerExt;
        let listener = listener.tap_io(|tcp| { let _ = tcp.set_nodelay(true); });
        run_bounded(async { axum::serve(listener, app).await.unwrap() }).await;
    } else {
        // As in a2a-rs's own examples/src/helloworld/server.rs.
        run_bounded(async { axum::serve(listener, app).await.unwrap() }).await;
    }
}
