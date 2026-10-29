// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One replica for the live scripts: JSON-RPC on `$PORT`, tasks in
//! PostgreSQL at `$DATABASE_URL`, `$NAME` in its log lines. Built against
//! this checkout and against v0.15.0 from this one file. What a message's
//! text asks for:
//!
//! * `hold`: working, then waits until cancelled (60 s at most).
//! * `park`: an artifact, 400 ms, another artifact, then input-required.
//! * `tick`: a part of artifact `tick` every 500 ms, 80 in all, then
//!   completed; stops when cancelled.
//! * `burst`, `burst6k`: 2,000 or 6,000 parts of `tick`, 50 at a time with
//!   50 ms between, then completed; logs how many writes failed.
//! * anything else: completed at once.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::serve::Server;
use a2a_protocol_server::store::PostgresTaskStore;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::TaskState;

type Fut<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>;

struct Exec(&'static str);

impl AgentExecutor for Exec {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            let text = ctx.message.text().unwrap_or_default().to_owned();
            emit.status(TaskState::Working).await?;
            match text.as_str() {
                "hold" => {
                    let fired = tokio::time::timeout(
                        Duration::from_secs(60),
                        ctx.cancellation_token.cancelled(),
                    )
                    .await
                    .is_ok();
                    eprintln!("[{}] hold ended, cancelled={fired}", self.0);
                }
                "park" => {
                    let early = format!("{}-early", self.0);
                    emit.artifact(&early, vec![Part::text("early")], None, Some(true))
                        .await?;
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    let late = format!("{}-late", self.0);
                    emit.artifact(&late, vec![Part::text("late")], None, Some(true))
                        .await?;
                    emit.status(TaskState::InputRequired).await?;
                }
                "burst" | "burst6k" => {
                    let n = if text == "burst6k" { 6000 } else { 2000 };
                    let started = std::time::Instant::now();
                    emit.artifact("tick", vec![Part::text("0")], Some(false), Some(false))
                        .await?;
                    let mut failed = 0;
                    for i in 1..n {
                        if i % 50 == 0 {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                        let part = vec![Part::text(i.to_string())];
                        if let Err(e) = emit.artifact("tick", part, Some(true), Some(false)).await {
                            failed += 1;
                            if failed == 1 {
                                let ms = started.elapsed().as_millis();
                                eprintln!("[{}] burst write {i} failed at {ms} ms: {e}", self.0);
                            }
                        }
                    }
                    let ms = started.elapsed().as_millis();
                    eprintln!(
                        "[{}] burst done after {ms} ms, {failed} writes failed",
                        self.0
                    );
                    emit.status(TaskState::Completed).await?;
                }
                "tick" => {
                    emit.artifact("tick", vec![Part::text("0")], Some(false), Some(false))
                        .await?;
                    for i in 1..80 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        if ctx.cancellation_token.is_cancelled() {
                            eprintln!("[{}] tick stopped at {i}, cancelled", self.0);
                            return Ok(());
                        }
                        let part = vec![Part::text(i.to_string())];
                        if let Err(e) = emit.artifact("tick", part, Some(true), Some(false)).await {
                            eprintln!("[{}] tick {i} not written: {e}", self.0);
                        }
                    }
                    emit.status(TaskState::Completed).await?;
                }
                _ => emit.status(TaskState::Completed).await?,
            }
            Ok(())
        })
    }
}

#[tokio::main]
async fn main() {
    let name: &'static str = Box::leak(std::env::var("NAME").unwrap_or_default().into_boxed_str());
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let store = PostgresTaskStore::new(&url).await.expect("store");
    let handler = Arc::new(
        RequestHandlerBuilder::new(Exec(name))
            .without_agent_card()
            .with_task_store(store)
            .build()
            .expect("handler"),
    );
    let port = std::env::var("PORT").expect("PORT");
    let server = Server::bind(&format!("127.0.0.1:{port}"))
        .await
        .expect("bind");
    eprintln!("[{name}] serving");
    let h = Arc::clone(&handler);
    server
        .serve_with_shutdown(JsonRpcDispatcher::new(handler), async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    let _ = h.shutdown().await;
}
