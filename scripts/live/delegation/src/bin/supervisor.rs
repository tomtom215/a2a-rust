// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The supervisor for `delegation_live.py`: delegates to the worker with a
//! `PostgresDelegationJournal`, and recovers what a dead supervisor left.
//!
//! ```text
//! a2a-live-supervisor start N    begin N journaled children, wait on them
//! a2a-live-supervisor recover    the book's recovery loop over journal.open(owner): the OLD,
//!                                serial one, reattach then wait, one record at a time
//! a2a-live-supervisor recover-all  the book's loop since 291aaf89: reattach every record,
//!                                then wait on all the children together
//! a2a-live-supervisor serve      the same supervisor as an agent behind
//!                                Server::serve_with_shutdown; a message
//!                                "fan-out N" starts N children
//! ```
//!
//! Environment: `WORKER_URL` (http://127.0.0.1:PORT), `DATABASE_URL` (the
//! journal's PostgreSQL), `A2A_OWNER` (the journal owner), and for `serve`
//! `PORT`. Each child is sent the message text `hold`. `HAND_OFF=off` (for
//! `start`) wires the shutdown signal to the cancel future instead, as the
//! book's pattern did before `wait_or_hand_off`: a control for the harness.
//! `LEASE_SECS` grants each child a lease that long, and `recover` gives
//! `reattach_with` the same lease.
//! `LINGER_SECS`: how long a failed `recover` or `recover-all` keeps the process alive
//! (so that the cancels of the handles it dropped can go out) before it exits 1.
//!
//! SIGTERM (and SIGINT) is the supervisor's shutdown signal, and every wait is
//! `Delegation::wait_or_hand_off(cancel, shutting_down)`: the documented
//! pattern for a graceful redeploy. In `start` the cancel future never fires,
//! as in the book's loop; in `serve` it is the executor's own
//! `cancellation_token`, which the server fires on shutdown after it stops
//! accepting. Output is one line per event, `t=<unix ms> KIND key=value ...`.

use std::future::Future;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use a2a_protocol_client::delegation::{Delegation, DelegationOptions, Outcome};
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::serve::Server;
use a2a_protocol_server::store::{PostgresDelegationJournal, PostgresTaskStore};
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder};
use a2a_protocol_types::delegation_journal::DelegationJournal;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::task::TaskState;
use a2a_protocol_types::{Message, MessageSendParams, Part};
use tokio::sync::watch;
use tokio::task::JoinSet;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;
type Fut<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

fn log(kind: &str, fields: &str) {
    println!("t={} {kind} {fields}", now_ms());
}

fn env(name: &str) -> Result<String, BoxErr> {
    std::env::var(name).map_err(|_| format!("{name} is not set").into())
}

/// A short label for how a delegation ended, for the driver to count.
fn label(outcome: &Outcome) -> String {
    let name = match outcome {
        Outcome::Completed(_) => "completed",
        Outcome::Failed { .. } => "failed",
        Outcome::Canceled(_) => "canceled",
        Outcome::Interrupted(_) => "interrupted",
        Outcome::Message(_) => "message",
        Outcome::Lost(_) => "lost",
        Outcome::Unreachable => "unreachable",
        Outcome::CancelRequested(_) => "cancel_requested",
        Outcome::CancelFailed(_) => "cancel_failed",
        Outcome::HandedOff => "handed_off",
        _ => "other",
    };
    if matches!(outcome, Outcome::Completed(_) | Outcome::HandedOff) {
        name.to_owned()
    } else {
        let detail: String = format!("{outcome:?}").chars().take(160).collect();
        format!("{name} detail={detail:?}")
    }
}

/// What every mode shares: the worker client, the journal, who owns records.
#[derive(Clone)]
struct Ctx {
    client: Arc<A2aClient>,
    journal: Arc<dyn DelegationJournal>,
    owner: String,
    agent: String,
    /// `LEASE_SECS`: grant each child a lease of this many seconds, and renew it
    /// again on reattach.
    lease: Option<std::time::Duration>,
}

impl Ctx {
    async fn from_env() -> Result<Self, BoxErr> {
        let agent = env("WORKER_URL")?;
        let pool = PostgresTaskStore::pool_options()
            .max_connections(10)
            .connect(&env("DATABASE_URL")?)
            .await?;
        Ok(Self {
            client: Arc::new(ClientBuilder::new(agent.clone()).build()?),
            journal: Arc::new(PostgresDelegationJournal::from_pool(pool).await?),
            owner: env("A2A_OWNER")?,
            agent,
            lease: std::env::var("LEASE_SECS").ok().and_then(|s| s.parse().ok()).map(std::time::Duration::from_secs),
        })
    }
}

/// Resolves when the process is told to shut down: the supervisor's own signal.
fn shutdown_signal() -> Result<(watch::Sender<bool>, watch::Receiver<bool>), BoxErr> {
    use tokio::signal::unix::{SignalKind, signal};
    let (tx, rx) = watch::channel(false);
    let (mut term, mut int) = (signal(SignalKind::terminate())?, signal(SignalKind::interrupt())?);
    let tx2 = tx.clone();
    tokio::spawn(async move {
        tokio::select! { _ = term.recv() => {}, _ = int.recv() => {} }
        log("SIGNAL", "shutdown signal received");
        let _ = tx2.send(true);
    });
    Ok((tx, rx))
}

async fn fired(mut rx: watch::Receiver<bool>) {
    let _ = rx.wait_for(|v| *v).await;
}

/// One child: start it journaled, learn its id, wait for it or hand it off.
async fn child<C, M>(i: usize, c: Ctx, shutdown: watch::Receiver<bool>, hand_off: bool, cancel: M) -> String
where
    C: Future<Output = ()>,
    M: FnOnce() -> C,
{
    let id = format!("{}-{}-{i}", c.owner, std::process::id());
    let params = MessageSendParams::new(Message::user(id, vec![Part::text("hold")]));
    let mut options = DelegationOptions::new().with_journal(c.journal, c.owner, c.agent);
    if let Some(ttl) = c.lease {
        options = options.with_lease(ttl);
    }
    let mut d = match Delegation::start_with(c.client, params, options).await {
        Ok(d) => d,
        Err(e) => {
            log("START_ERR", &format!("i={i} error={:?}", e.to_string()));
            return "start_err".into();
        }
    };
    let record = d.journal_id().unwrap_or("-").to_owned();
    log("STARTED", &format!("i={i} record={record}"));
    // What `cancel()` does to learn the id: read until an event names the child.
    while d.task_id().is_none() && !d.is_settled() {
        if !matches!(d.next_event().await, Some(Ok(_))) {
            break;
        }
    }
    log("NAMED", &format!("i={i} record={record} task={}", d.task_id().unwrap_or("-")));
    // `hand_off == false` is the control: the shutdown signal is wired to nothing, as when
    // a supervisor passes it to `wait` and not `wait_or_hand_off`.
    let signal = async move {
        if hand_off {
            fired(shutdown).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    let done = d.wait_or_hand_off(cancel(), signal).await;
    let out = label(&done.outcome);
    log("OUTCOME", &format!("i={i} record={record} task={} outcome={out}", done.task_id.as_deref().unwrap_or("-")));
    out.split(' ').next().unwrap_or("").to_owned()
}

/// Starts `n` children and waits for all of them; prints a count per outcome.
async fn run_children<C, M>(n: usize, c: &Ctx, shutdown: &watch::Receiver<bool>, hand_off: bool, cancel: M) -> Vec<String>
where
    C: Future<Output = ()> + Send + 'static,
    M: Fn() -> C + Clone + Send + 'static,
{
    let mut set = JoinSet::new();
    for i in 0..n {
        set.spawn(child(i, c.clone(), shutdown.clone(), hand_off, cancel.clone()));
    }
    let mut labels = Vec::new();
    while let Some(done) = set.join_next().await {
        labels.push(done.unwrap_or_else(|e| format!("panic {e}")));
    }
    labels.sort();
    let mut summary = String::new();
    for l in labels.iter().collect::<std::collections::BTreeSet<_>>() {
        summary += &format!("{l}={} ", labels.iter().filter(|x| *x == l).count());
    }
    log("SUMMARY", &format!("children={n} {summary}"));
    labels
}

/// The book's recovery loop (book/src/client/delegation.md, "Remembering
/// children across restarts"), with a line printed per record.
async fn recover(c: Ctx) -> Result<(), BoxErr> {
    let open = c.journal.open(&c.owner).await?;
    let named = open.iter().filter(|r| r.task_id.is_some()).count();
    log("RECOVER", &format!("owner={} open={} with_task_id={named}", c.owner, open.len()));
    for record in open {
        if let Some(task_id) = record.task_id.clone() {
            log("REATTACH", &format!("record={} task={task_id} last_event_id={:?}", record.id, record.last_event_id));
            let mut options = DelegationOptions::new().with_journal_record(Arc::clone(&c.journal), &record.id);
            if let Some(ttl) = c.lease {
                options = options.with_lease(ttl); // the book: a restart must give reattach_with the lease
            }
            let child = Delegation::reattach_with(
                Arc::clone(&c.client), record.tenant.clone(), task_id, record.last_event_id.clone(), options,
            )
            .await?;
            let done = child.wait(std::future::pending()).await;
            log("OUTCOME", &format!("record={} task={} outcome={}", record.id, done.task_id.as_deref().unwrap_or("-"), label(&done.outcome)));
        } else {
            // A record with no task id died before the child was named: it would be
            // resent under `record.idempotency_key` (the book). Not resent here.
            log("NO_TASK_ID", &format!("record={} idempotency_key={:?}", record.id, record.idempotency_key));
        }
    }
    log("RECOVERED", "loop done");
    Ok(())
}

/// The book's recovery loop as it reads since 291aaf89 (book/src/client/delegation.md):
/// reattach every open record first, then wait on all the children together. A
/// child's handle is what renews its lease, so the serial loop above leaves every
/// child but the one it waits on unrenewed. Lines are printed per record, and
/// `REATTACHED` says how long reattaching them all took.
async fn recover_all(c: Ctx) -> Result<(), BoxErr> {
    let began = std::time::Instant::now();
    let open = c.journal.open(&c.owner).await?;
    let named = open.iter().filter(|r| r.task_id.is_some()).count();
    log("RECOVER", &format!("owner={} open={} with_task_id={named}", c.owner, open.len()));
    let mut children = Vec::new();
    let mut record_of = std::collections::HashMap::new();
    for record in open {
        if let Some(task_id) = record.task_id.clone() {
            log("REATTACH", &format!("record={} task={task_id} last_event_id={:?}", record.id, record.last_event_id));
            let mut options = DelegationOptions::new().with_journal_record(Arc::clone(&c.journal), &record.id);
            if let Some(ttl) = c.lease {
                options = options.with_lease(ttl);
            }
            record_of.insert(task_id.clone(), record.id.clone());
            children.push(
                Delegation::reattach_with(
                    Arc::clone(&c.client), record.tenant.clone(), task_id, record.last_event_id.clone(), options,
                )
                .await?,
            );
        } else {
            log("NO_TASK_ID", &format!("record={} idempotency_key={:?}", record.id, record.idempotency_key));
        }
    }
    log("REATTACHED", &format!("children={} took_ms={}", children.len(), began.elapsed().as_millis()));
    let mut waiting = tokio::task::JoinSet::new();
    for child in children {
        waiting.spawn(child.wait(std::future::pending()));
    }
    while let Some(done) = waiting.join_next().await {
        let done = done?;
        let task = done.task_id.as_deref().unwrap_or("-");
        let record = done.task_id.as_ref().and_then(|t| record_of.get(t)).map_or("-", String::as_str);
        log("OUTCOME", &format!("record={record} task={task} outcome={}", label(&done.outcome)));
    }
    log("RECOVERED", "loop done");
    Ok(())
}

/// The supervisor as an agent: a message "fan-out N" starts N journaled
/// children, each waited on with the executor's own cancellation token as
/// `cancel` and the process's shutdown signal as `hand_off`.
struct Agent {
    ctx: Ctx,
    shutdown: watch::Receiver<bool>,
}

impl AgentExecutor for Agent {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a, A2aResult<()>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            let text = ctx.message.text().unwrap_or_default().to_owned();
            let n: usize = text.strip_prefix("fan-out ").and_then(|s| s.parse().ok()).unwrap_or(0);
            let token = ctx.cancellation_token.clone();
            let cancel = move || token.clone().cancelled_owned();
            let labels = run_children(n, &self.ctx, &self.shutdown, true, cancel).await;
            log("EXECUTOR_DONE", &format!("children={} token_cancelled={}", labels.len(), ctx.cancellation_token.is_cancelled()));
            emit.status(TaskState::Completed).await
        })
    }
}

async fn serve(c: Ctx) -> Result<(), BoxErr> {
    let (tx, rx) = shutdown_signal()?;
    let handler = Arc::new(
        RequestHandlerBuilder::new(Agent { ctx: c, shutdown: rx.clone() })
            .without_agent_card()
            .build()?,
    );
    let server = Server::bind(&format!("127.0.0.1:{}", env("PORT")?)).await?;
    log("SERVING", "supervisor agent listening");
    let h = Arc::clone(&handler);
    let report = server
        .serve_with_shutdown(JsonRpcDispatcher::new(handler), async move {
            let mut rx = tx.subscribe();
            let _ = rx.wait_for(|v| *v).await;
        })
        .await;
    let _ = h.shutdown().await;
    log("SERVED", &format!("report={report:?}"));
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), BoxErr> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let c = Ctx::from_env().await?;
    match args.first().map(String::as_str) {
        Some("start") => {
            let n: usize = args.get(1).and_then(|s| s.parse().ok()).ok_or("usage: start N")?;
            let (_tx, rx) = shutdown_signal()?;
            log("BEGIN", &format!("owner={} children={n} pid={}", c.owner, std::process::id()));
            // HAND_OFF=off is the control: the old pattern, `wait(shutdown)`: the signal cancels.
            if std::env::var("HAND_OFF").is_ok_and(|v| v == "off") {
                let signal = rx.clone();
                run_children(n, &c, &rx, false, move || fired(signal.clone())).await;
            } else {
                run_children(n, &c, &rx, true, || std::future::pending::<()>()).await;
            }
        }
        Some(mode @ ("recover" | "recover-all")) => {
            let done = if mode == "recover" { recover(c).await } else { recover_all(c).await };
            if let Err(e) = done {
                log("ERROR", &format!("{mode} failed: {e}"));
                // A supervisor goes on living after a failed recovery, and the handles the failure
                // dropped cancel their children in the background. LINGER_SECS stands in for that life.
                let linger = std::env::var("LINGER_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
                tokio::time::sleep(std::time::Duration::from_secs(linger)).await;
                log("EXIT", "after a failed recovery");
                std::process::exit(1);
            }
        }
        Some("serve") => serve(c).await?,
        _ => return Err("usage: a2a-live-supervisor start N | recover | recover-all | serve".into()),
    }
    Ok(())
}
