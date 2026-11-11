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
//! * `ask <question>`: a real model answers, through the SDK's tool loop (no
//!   tools), from the OpenAI-compatible server at `$C_MODEL_URL` (default
//!   `http://127.0.0.1:11434/v1`) as `$C_MODEL` (default `qwen3.5:2b`), thinking
//!   off and 400 tokens at most. The answer takes seconds to minutes on a CPU,
//!   which is what a kill-the-owner script needs.
//! * anything else: completed at once.
//!
//! `A2A_OUTBOX=1` gives the replica the durable push outbox in the same
//! database, push configs in that database too (so another replica can
//! deliver what this one queued), a sender that allows loopback webhooks, and
//! a store-write retry of 1 s (a reservation's hold is then about 63 s).
//! `A2A_KILL_AT=reserve|arm` (with `A2A_KILL_NTH`, default 1) makes the
//! replica send itself SIGKILL at the Nth outbox `reserve` (after the rows are
//! reserved, before the event is stored) or `arm` (after the event is stored,
//! before its rows are armed): the two ends of the window audit H14 closes.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::push::outbox::{ClaimedDelivery, Held, NewDelivery, OutboxFuture};
use a2a_protocol_server::push::{HttpPushSender, PostgresPushOutbox, PushOutbox};
use a2a_protocol_server::PostgresPushConfigStore;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::serve::Server;
use a2a_protocol_server::tool_loop::{OpenAiChatModel, ToolLoop, ToolLoopExecutor, ToolSet};
use a2a_protocol_server::store::{LogListen, PostgresTaskStore};
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::TaskState;

type Fut<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>;

struct Exec(&'static str, ToolLoopExecutor);

impl AgentExecutor for Exec {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a> {
        Box::pin(async move {
            let text = ctx.message.text().unwrap_or_default().to_owned();
            if text.starts_with("ask ") {
                return self.1.execute(ctx, queue).await;
            }
            let emit = EventEmitter::new(ctx, queue);
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

/// The PostgreSQL outbox, which sends its own process SIGKILL at one call:
/// no destructor, no flush, the way a replica is lost.
struct Killing {
    inner: PostgresPushOutbox,
    name: &'static str,
    at: String,
    nth: usize,
    calls: std::sync::atomic::AtomicUsize,
}

impl Killing {
    /// Whether this is the call to die at.
    fn dies_at(&self, call: &str) -> bool {
        self.at == call
            && self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 == self.nth
    }

    fn die(&self, call: &str) -> ! {
        eprintln!("[{}] SIGKILL at {call} #{}", self.name, self.nth);
        let _ = std::process::Command::new("kill")
            .args(["-9", &std::process::id().to_string()])
            .status();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

impl PushOutbox for Killing {
    fn record(&self, deliveries: Vec<NewDelivery>, per_webhook: usize) -> OutboxFuture<'_, usize> {
        self.inner.record(deliveries, per_webhook)
    }
    fn claim<'a>(
        &'a self,
        owner: &'a str,
        max: usize,
        lease: Duration,
    ) -> OutboxFuture<'a, Vec<ClaimedDelivery>> {
        self.inner.claim(owner, max, lease)
    }
    fn complete(&self, seq: i64) -> OutboxFuture<'_, ()> {
        self.inner.complete(seq)
    }
    fn retry_after<'a>(
        &'a self,
        seq: i64,
        owner: &'a str,
        after: Duration,
    ) -> OutboxFuture<'a, ()> {
        self.inner.retry_after(seq, owner, after)
    }
    fn pending(&self) -> OutboxFuture<'_, usize> {
        self.inner.pending()
    }
    fn supports_reservations(&self) -> bool {
        true
    }
    fn reserve<'a>(
        &'a self,
        reservation: &'a str,
        log_seq: u64,
        deliveries: Vec<NewDelivery>,
        per_webhook: usize,
        hold: Duration,
    ) -> OutboxFuture<'a, usize> {
        Box::pin(async move {
            let reserved = self
                .inner
                .reserve(reservation, log_seq, deliveries, per_webhook, hold)
                .await;
            if self.dies_at("reserve") {
                self.die("reserve");
            }
            reserved
        })
    }
    fn arm<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, usize> {
        Box::pin(async move {
            if self.dies_at("arm") {
                self.die("arm");
            }
            self.inner.arm(reservation).await
        })
    }
    fn release<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, ()> {
        self.inner.release(reservation)
    }
    fn held(&self, limit: usize) -> OutboxFuture<'_, Vec<Held>> {
        self.inner.held(limit)
    }
}

#[tokio::main]
async fn main() {
    let name: &'static str = Box::leak(std::env::var("NAME").unwrap_or_default().into_boxed_str());
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    // `A2A_POOL_SIZE` sizes the task store's pool; `A2A_LOG_NOTIFY` (`pool` or
    // `direct`) wakes a cross-replica stream from the event log's notifications
    // (`PostgresTaskStore::with_log_notifications`, audit E-F25 (b)).
    let size = std::env::var("A2A_POOL_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(a2a_protocol_server::store::DEFAULT_POOL_SIZE);
    let mut store = PostgresTaskStore::new_with_pool_size(&url, size)
        .await
        .expect("store");
    store = match std::env::var("A2A_LOG_NOTIFY").as_deref() {
        Ok("pool") => store.with_log_notifications(LogListen::Pool),
        Ok("direct") => store.with_log_notifications(LogListen::Direct(url.clone())),
        _ => store,
    };
    let base = std::env::var("C_MODEL_URL").unwrap_or_else(|_| "http://127.0.0.1:11434/v1".into());
    let model_name = std::env::var("C_MODEL").unwrap_or_else(|_| "qwen3.5:2b".into());
    let mut extra = serde_json::Map::new();
    extra.insert(
        "chat_template_kwargs".to_owned(),
        serde_json::json!({"enable_thinking": false}),
    );
    extra.insert("max_tokens".to_owned(), serde_json::json!(400));
    let model = OpenAiChatModel::new(&base, model_name)
        .expect("model")
        .with_extra_body(extra);
    let asker = ToolLoopExecutor::new(
        ToolLoop::new(model, ToolSet::new())
            .with_preamble("Answer the question in about three hundred words."),
    );
    let mut builder = RequestHandlerBuilder::new(Exec(name, asker))
        .without_agent_card()
        .with_task_store(store);
    if std::env::var("A2A_OUTBOX").as_deref() == Ok("1") {
        let outbox = PostgresPushOutbox::new(&url).await.expect("outbox");
        let outbox: Arc<dyn PushOutbox> = match std::env::var("A2A_KILL_AT") {
            Ok(at) => Arc::new(Killing {
                inner: outbox,
                name,
                at,
                nth: std::env::var("A2A_KILL_NTH")
                    .ok()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1),
                calls: std::sync::atomic::AtomicUsize::new(0),
            }),
            Err(_) => Arc::new(outbox),
        };
        builder = builder
            .with_push_config_store(PostgresPushConfigStore::new(&url).await.expect("configs"))
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .with_push_outbox(outbox)
            .with_handler_limits(
                a2a_protocol_server::HandlerLimits::default()
                    .with_store_write_retry(Duration::from_secs(1)),
            );
    }
    let handler = Arc::new(builder.build().expect("handler"));
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
