// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The worker for `delegation_live.py`: JSON-RPC on `$PORT`, tasks in
//! PostgreSQL at `$DATABASE_URL`, `$NAME` in its log lines.
//!
//! What a message's text asks for:
//!
//! * `hold`: working for `$HOLD_SECS` seconds (60 by default), then
//!   **completed**; stops at once when cancelled. This is not
//!   `scripts/live/replica`'s `hold`, which returns after its 60 s without a
//!   terminal state and so leaves the task `working` for good (the server logs
//!   that as `left_unsettled`; the executor guide says so). A child that never
//!   completes cannot show a recovery run completing it.
//! * anything else: completed at once.
//!
//! `LEASE=1` makes the worker grant leases (an `InMemoryLeaseStore`).
//!
//! Every start, completion and cancellation is one log line with a Unix
//! millisecond timestamp, so a run can count the cancels it saw: `cancel-hook`
//! is the server asking the executor to cancel (`CancelTask`), `token-fired`
//! is the executor seeing its token.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::serve::Server;
use a2a_protocol_server::store::{InMemoryLeaseStore, PostgresTaskStore};
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::task::TaskState;

type Fut<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

struct Exec {
    name: &'static str,
    hold: Duration,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

impl AgentExecutor for Exec {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a, A2aResult<()>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            let text = ctx.message.text().unwrap_or_default().to_owned();
            let (name, id) = (self.name, &ctx.task_id);
            eprintln!("[{name}] t={} start task={id} text={text}", now_ms());
            if text != "hold" {
                return emit.status(TaskState::Completed).await;
            }
            emit.status(TaskState::Working).await?;
            let cancelled = tokio::time::timeout(self.hold, ctx.cancellation_token.cancelled())
                .await
                .is_ok();
            if cancelled {
                eprintln!("[{name}] t={} token-fired task={id}", now_ms());
                return Ok(());
            }
            emit.status(TaskState::Completed).await?;
            eprintln!("[{name}] t={} completed task={id}", now_ms());
            Ok(())
        })
    }

    // Logs, and writes nothing: the handler ends the task `canceled` after a
    // hook that wrote no terminal state (`TerminalTracking::cancel_with`).
    fn cancel<'a>(&'a self, ctx: &'a RequestContext, _queue: &'a dyn EventQueueWriter) -> Fut<'a, A2aResult<()>> {
        Box::pin(async move {
            eprintln!("[{}] t={} cancel-hook task={}", self.name, now_ms(), ctx.task_id);
            Ok(())
        })
    }
}

#[tokio::main]
async fn main() {
    let name: &'static str = Box::leak(std::env::var("NAME").unwrap_or_default().into_boxed_str());
    let hold = std::env::var("HOLD_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let store = PostgresTaskStore::new(&url).await.expect("store");
    let mut builder = RequestHandlerBuilder::new(Exec { name, hold: Duration::from_secs(hold) })
        .without_agent_card()
        .with_task_store(store);
    // LEASE=1: the worker grants leases (ADR 0020) and stops a child whose lease lapses.
    if std::env::var("LEASE").is_ok_and(|v| v == "1") {
        builder = builder.with_lease_store(InMemoryLeaseStore::new());
    }
    let handler = Arc::new(builder.build().expect("handler"));
    let port = std::env::var("PORT").expect("PORT");
    let server = Server::bind(&format!("127.0.0.1:{port}")).await.expect("bind");
    eprintln!("[{name}] t={} serving hold_secs={hold}", now_ms());
    let h = Arc::clone(&handler);
    let _report = server
        .serve_with_shutdown(JsonRpcDispatcher::new(handler), async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    let _ = h.shutdown().await;
    eprintln!("[{name}] t={} stopped", now_ms());
}
