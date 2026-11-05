// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Audit H4 (b): does a quiet child survive over gRPC when the client's idle
//! timeout cuts its stream again and again?
//!
//! One process: this repository's gRPC server (`GrpcDispatcher`, in-memory
//! task store) with an executor that emits `working`, says nothing for
//! `$QUIET_SECS` (90 by default), then `completed`; and this repository's
//! client over gRPC with `stream_idle_timeout` set to `$IDLE_SECS` (10 by
//! default), following it with `Delegation::start` and `wait`. gRPC streams
//! carry no event ids, so every cut is resumed from the snapshot, and the
//! delegation survives them only through the 2026-11-03 rule that a stream which
//! delivered an event and stayed open for `healthy_after` (5 s) worked. Before
//! that rule a delegation was given up on after about six cuts and the child
//! cancelled. A server interceptor logs every call it receives, with a
//! timestamp: the `SubscribeToTask` lines are the resumes, a `CancelTask`
//! line would be the cancel. Prints one `RESULT` line.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use a2a_protocol_client::config::BINDING_GRPC;
use a2a_protocol_client::delegation::{Delegation, Outcome};
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::interceptor::ServerInterceptor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::task::TaskState;
use a2a_protocol_types::{Message, MessageSendParams, Part};

type Fut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

fn secs(name: &str, default: u64) -> Duration {
    Duration::from_secs(std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default))
}

struct Quiet(Duration);

impl AgentExecutor for Quiet {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a, A2aResult<()>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            println!("t={} CHILD working task={}, quiet for {:?}", now_ms(), ctx.task_id, self.0);
            if tokio::time::timeout(self.0, ctx.cancellation_token.cancelled()).await.is_ok() {
                println!("t={} CHILD token-fired task={}", now_ms(), ctx.task_id);
                return Ok(());
            }
            println!("t={} CHILD completing task={}", now_ms(), ctx.task_id);
            emit.status(TaskState::Completed).await
        })
    }
}

/// Logs and counts every call the server receives.
struct Calls(Arc<Mutex<BTreeMap<String, u32>>>);

impl ServerInterceptor for Calls {
    fn before<'a>(&'a self, ctx: &'a CallContext) -> Fut<'a, A2aResult<()>> {
        Box::pin(async move {
            println!("t={} RPC {}", now_ms(), ctx.method());
            *self.0.lock().unwrap().entry(ctx.method().to_owned()).or_default() += 1;
            Ok(())
        })
    }

    fn after<'a>(&'a self, _ctx: &'a CallContext) -> Fut<'a, A2aResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (quiet, idle) = (secs("QUIET_SECS", 90), secs("IDLE_SECS", 10));
    let calls = Arc::new(Mutex::new(BTreeMap::new()));
    let handler = Arc::new(
        RequestHandlerBuilder::new(Quiet(quiet))
            .without_agent_card()
            .with_interceptor(Calls(Arc::clone(&calls)))
            .build()?,
    );
    let addr = GrpcDispatcher::new(handler, GrpcConfig::default()).serve_with_addr("127.0.0.1:0").await?;
    let client = Arc::new(
        ClientBuilder::new(format!("http://{addr}"))
            .with_protocol_binding(BINDING_GRPC)
            .with_stream_idle_timeout(Some(idle))
            .build_grpc()
            .await?,
    );
    println!("t={} BEGIN grpc={addr} quiet={quiet:?} client_stream_idle_timeout={idle:?}", now_ms());
    let started = Instant::now();
    let params = MessageSendParams::new(Message::user("idle-1", vec![Part::text("quiet")]));
    let done = Delegation::start(client, params).await?.wait(std::future::pending()).await;
    let label = match &done.outcome {
        Outcome::Completed(_) => "completed".to_owned(),
        other => format!("{other:?}").chars().take(200).collect(),
    };
    let calls = calls.lock().unwrap().clone();
    println!(
        "t={} RESULT outcome={label} elapsed_s={:.1} quiet_s={} idle_s={} calls={calls:?}",
        now_ms(), started.elapsed().as_secs_f32(), quiet.as_secs(), idle.as_secs()
    );
    Ok(())
}
