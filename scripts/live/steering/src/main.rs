// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The agent for `steering_live.py`: JSON-RPC on `$PORT`, tasks in PostgreSQL
//! at `$DATABASE_URL`, `$NAME` in its log lines, steering on (ADR 0036).
//!
//! What the message that starts a task asks for:
//!
//! * `read`: working; reads its inbox, logs each message it reads (who sent
//!   it, what it says), and completes when one says `done`.
//! * `readlate`: as `read`, but reads nothing until `$LATE_SECS` (3) seconds
//!   have passed, so what arrives meanwhile waits in the inbox and behind it.
//! * `hold`: working; never reads its inbox; works until cancelled (60 s).
//! * anything else: completed at once.
//!
//! Settings: `STEER_CAP` (inbox capacity, 16), `STEER_NOTIFY` (`1`: announce
//! a handed-off message with `NOTIFY`), `RENEWAL_MS` (claim renewal, 1000) and
//! `TTL_MS` (claim time to live, 15000).
//!
//! Every event is one log line with a Unix millisecond timestamp, so the
//! driver can measure from a response to the executor's read.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::serve::Server;
use a2a_protocol_server::store::PostgresTaskStore;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder, SteeringConfig};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::task::TaskState;

type Fut<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

struct Exec {
    name: &'static str,
    late: Duration,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

fn env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

impl AgentExecutor for Exec {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a, A2aResult<()>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            let text = ctx.message.text().unwrap_or_default().to_owned();
            let (name, id) = (self.name, &ctx.task_id);
            eprintln!("[{name}] t={} start task={id} text={text}", now_ms());
            if !matches!(text.as_str(), "read" | "readlate" | "hold") {
                return emit.status(TaskState::Completed).await;
            }
            emit.status(TaskState::Working).await?;
            if text == "hold" {
                let _ = tokio::time::timeout(Duration::from_secs(60), ctx.cancellation_token.cancelled()).await;
                eprintln!("[{name}] t={} hold ended task={id}", now_ms());
                return Ok(());
            }
            if text == "readlate" {
                tokio::time::sleep(self.late).await;
            }
            let inbox = ctx.inbox().expect("steering is on");
            loop {
                tokio::select! {
                    biased;
                    () = ctx.cancellation_token.cancelled() => {
                        eprintln!("[{name}] t={} token-fired task={id}", now_ms());
                        return Ok(());
                    }
                    steered = inbox.recv() => {
                        let Some(steered) = steered else { return Ok(()) };
                        let said = steered.message.text().unwrap_or_default().to_owned();
                        let from = steered.from.as_deref().unwrap_or("-");
                        eprintln!("[{name}] t={} read task={id} from={from} text={said}", now_ms());
                        if said == "done" {
                            eprintln!("[{name}] t={} completed task={id}", now_ms());
                            return emit.status(TaskState::Completed).await;
                        }
                    }
                }
            }
        })
    }
}

#[tokio::main]
async fn main() {
    let name: &'static str = Box::leak(std::env::var("NAME").unwrap_or_default().into_boxed_str());
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let store = PostgresTaskStore::new(&url).await.expect("store");
    let config = SteeringConfig::new()
        .with_inbox_capacity(env("STEER_CAP", 16))
        .with_notifications(env::<u8>("STEER_NOTIFY", 0) == 1);
    let renewal = Duration::from_millis(env("RENEWAL_MS", 1000));
    let ttl = Duration::from_millis(env("TTL_MS", 15_000));
    let late = Duration::from_secs(env("LATE_SECS", 3));
    let handler = Arc::new(
        RequestHandlerBuilder::new(Exec { name, late })
            .without_agent_card()
            .with_task_store(store)
            .with_turn_claim_timing(ttl, renewal)
            .with_replica_name(name)
            .with_steering(config)
            .build()
            .expect("handler"),
    );
    let port = std::env::var("PORT").expect("PORT");
    let server = Server::bind(&format!("127.0.0.1:{port}")).await.expect("bind");
    eprintln!("[{name}] t={} serving cap={} notify={} renewal_ms={} ttl_ms={}",
        now_ms(), config.inbox_capacity(), config.notifications(), renewal.as_millis(), ttl.as_millis());
    let h = Arc::clone(&handler);
    let _report = server
        .serve_with_shutdown(JsonRpcDispatcher::new(handler), async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    let _ = h.shutdown().await;
    eprintln!("[{name}] t={} stopped", now_ms());
}
