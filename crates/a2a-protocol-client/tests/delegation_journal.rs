// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The delegation journal (ADR 0030): the record is written before the child
//! exists, follows it, and is settled however the delegation ends; a
//! restarted supervisor picks its open children up from the records.
//!
//! The server is this repository's, over real sockets.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use a2a_protocol_client::delegation::{Delegation, Outcome, ResumePolicy};
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::store::TenantAwareInMemoryTaskStore;
use a2a_protocol_server::{Dispatcher, EventEmitter, agent_executor};
use a2a_protocol_types::delegation_journal::{
    DelegationJournal, DelegationRecord, InMemoryDelegationJournal,
};
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::idempotency;
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskState};
use tokio::net::TcpListener;
use tokio::sync::Notify;

const GUARD: Duration = Duration::from_secs(20);

/// What a `hold:` child waits for: one gate per message text.
static GATES: LazyLock<Mutex<HashMap<String, Arc<Notify>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn gate(key: &str) -> Arc<Notify> {
    Arc::clone(
        GATES
            .lock()
            .unwrap()
            .entry(key.to_owned())
            .or_insert_with(|| Arc::new(Notify::new())),
    )
}

/// Orders the journal's `begin` against the executor's start.
static CLOCK: AtomicUsize = AtomicUsize::new(1);
static STARTS: LazyLock<Mutex<HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Worker;
agent_executor!(Worker, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    let text = ctx.message.text().unwrap_or_default().to_owned();
    STARTS
        .lock()
        .unwrap()
        .insert(text.clone(), CLOCK.fetch_add(1, Ordering::SeqCst));
    if text.starts_with("hold:") {
        // Works until the test opens its gate, or is cancelled.
        emit.status(TaskState::Working).await?;
        let released = gate(&text);
        tokio::select! {
            () = released.notified() => emit.status(TaskState::Completed).await,
            () = ctx.cancellation_token.cancelled() => Ok(()),
        }
    } else if text == "abandon" {
        // Starts the task, then returns without settling it.
        emit.status(TaskState::Working).await
    } else if text.starts_with("tick:") {
        // A stream of events: sixteen artifact chunks 100 ms apart, then done.
        emit.status(TaskState::Working).await?;
        for i in 0..16 {
            emit.artifact(
                "log",
                vec![Part::text(format!("line {i}"))],
                Some(true),
                None,
            )
            .await?;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        emit.status(TaskState::Completed).await
    } else {
        emit.status(TaskState::Working).await?;
        emit.status(TaskState::Completed).await
    }
});

async fn serve() -> SocketAddr {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Worker)
            .without_agent_card()
            .with_task_store(TenantAwareInMemoryTaskStore::new())
            .build()
            .expect("handler"),
    );
    let dispatcher: Arc<dyn Dispatcher> = Arc::new(JsonRpcDispatcher::new(handler));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let io = hyper_util::rt::TokioIo::new(stream);
            let d = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&d);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
    addr
}

fn client_for(addr: SocketAddr) -> Arc<A2aClient> {
    Arc::new(
        ClientBuilder::new(format!("http://{addr}"))
            .build()
            .expect("client"),
    )
}

fn job(text: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user(
        uuid::Uuid::new_v4().to_string(),
        vec![Part::text(text)],
    ))
}

fn key() -> String {
    format!("hold:{}", uuid::Uuid::new_v4())
}

/// Reads events until the child is named.
async fn until_named(d: &mut Delegation) -> String {
    tokio::time::timeout(GUARD, async {
        while d.task_id().is_none() {
            d.next_event()
                .await
                .expect("an event")
                .expect("not an error");
        }
    })
    .await
    .expect("named in time");
    d.task_id().expect("named").to_owned()
}

/// Wraps the in-memory journal: stamps `begin`, counts `progress`, and can be
/// told to refuse `begin`.
struct Probe {
    inner: InMemoryDelegationJournal,
    began_at: AtomicUsize,
    progress_calls: AtomicUsize,
    refuse_begin: bool,
}

impl Probe {
    fn new(refuse_begin: bool) -> Arc<Self> {
        Arc::new(Self {
            inner: InMemoryDelegationJournal::new(),
            began_at: AtomicUsize::new(0),
            progress_calls: AtomicUsize::new(0),
            refuse_begin,
        })
    }
}

type Fut<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<T>> + Send + 'a>>;

impl DelegationJournal for Probe {
    fn begin<'a>(&'a self, record: &'a DelegationRecord) -> Fut<'a, ()> {
        if self.refuse_begin {
            return Box::pin(async { Err(A2aError::internal("the journal is full")) });
        }
        self.began_at
            .store(CLOCK.fetch_add(1, Ordering::SeqCst), Ordering::SeqCst);
        self.inner.begin(record)
    }

    fn progress<'a>(
        &'a self,
        id: &'a str,
        task_id: Option<&'a str>,
        last_event_id: Option<&'a str>,
        now_ms: i64,
    ) -> Fut<'a, ()> {
        self.progress_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.progress(id, task_id, last_event_id, now_ms)
    }

    fn settle<'a>(&'a self, id: &'a str, outcome: &'a str, now_ms: i64) -> Fut<'a, ()> {
        self.inner.settle(id, outcome, now_ms)
    }

    fn open<'a>(&'a self, owner: &'a str) -> Fut<'a, Vec<DelegationRecord>> {
        self.inner.open(owner)
    }

    fn purge_settled(&self, before: i64) -> Fut<'_, u64> {
        self.inner.purge_settled(before)
    }
}

async fn start(
    client: &Arc<A2aClient>,
    params: MessageSendParams,
    journal: &Arc<Probe>,
) -> Delegation {
    Delegation::start_journaled(
        Arc::clone(client),
        params,
        Arc::clone(journal) as Arc<dyn DelegationJournal>,
        "supervisor-1",
        "http://worker.example",
    )
    .await
    .expect("start")
}

fn keyed(text: &str, key: &str) -> MessageSendParams {
    let mut params = job(text);
    idempotency::set_key(&mut params.message, key).expect("a valid key");
    params
}

#[tokio::test]
async fn the_record_is_written_before_the_child_exists_and_settled_with_its_outcome() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    let key = "k".repeat(32);
    let text = format!("done:{}", uuid::Uuid::new_v4());
    let d = start(&client, keyed(&text, &key), &journal).await;
    let record_id = d.journal_id().expect("journaled").to_owned();
    let done = d.wait(std::future::pending()).await;
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");

    assert!(
        journal.began_at.load(Ordering::SeqCst) < STARTS.lock().unwrap()[&text],
        "begin ran before the executor started"
    );
    let r = journal.inner.get(&record_id).expect("recorded");
    assert_eq!(r.owner, "supervisor-1");
    assert_eq!(r.agent, "http://worker.example");
    assert_eq!(r.idempotency_key.as_deref(), Some(key.as_str()));
    assert_eq!(r.task_id, done.task_id, "the child's id");
    assert_eq!(r.outcome.as_deref(), Some("completed"));
    assert!(r.updated_at_ms >= r.started_at_ms);
}

#[tokio::test]
async fn a_journal_that_cannot_record_starts_nothing() {
    let client = client_for(serve().await);
    let journal = Probe::new(true);
    let text = format!("done:{}", uuid::Uuid::new_v4());
    let err = Delegation::start_journaled(
        Arc::clone(&client),
        job(&text),
        journal as Arc<dyn DelegationJournal>,
        "supervisor-1",
        "http://worker.example",
    )
    .await
    .expect_err("refused");
    assert!(err.to_string().contains("the journal is full"), "{err}");
    // No message was sent, so no executor started.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!STARTS.lock().unwrap().contains_key(&text));
}

#[tokio::test]
async fn a_restarted_supervisor_finds_its_open_child_and_picks_it_up() {
    let upstream = serve().await;
    let journal = Probe::new(false);
    let hold = key();

    // The first supervisor starts the child, then goes away without ending it.
    let mut first = start(&client_for(upstream), job(&hold), &journal).await;
    let id = until_named(&mut first).await;
    let record_id = first.journal_id().unwrap().to_owned();
    assert_eq!(first.detach().as_deref(), Some(id.as_str()));

    // The second finds it in the journal: open, named, and not settled.
    let open = journal.open("supervisor-1").await.expect("open");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, record_id);
    assert_eq!(open[0].task_id.as_deref(), Some(id.as_str()));
    assert!(journal.open("someone-else").await.expect("open").is_empty());

    let again = Delegation::reattach(
        client_for(upstream),
        open[0].tenant.clone(),
        id.clone(),
        open[0].last_event_id.clone(),
    )
    .await
    .expect("reattached")
    .with_journal(
        Arc::clone(&journal) as Arc<dyn DelegationJournal>,
        &open[0].id,
    );
    gate(&hold).notify_one();
    let done = tokio::time::timeout(GUARD, again.wait(std::future::pending()))
        .await
        .expect("bounded");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
    assert_eq!(
        journal.inner.get(&record_id).unwrap().outcome.as_deref(),
        Some("completed"),
        "the record the first supervisor began is settled by the second"
    );
    assert!(journal.open("supervisor-1").await.expect("open").is_empty());
}

#[tokio::test]
async fn every_way_a_delegation_ends_settles_its_record() {
    let upstream = serve().await;
    let client = client_for(upstream);

    // Lost, with resuming off: the child abandons its task, the stream ends.
    let journal = Probe::new(false);
    let d = start(&client, job("abandon"), &journal)
        .await
        .with_resume(ResumePolicy::none());
    let record = d.journal_id().unwrap().to_owned();
    let done = d.wait(std::future::pending()).await;
    assert!(matches!(done.outcome, Outcome::Lost(_)));
    assert_eq!(
        journal.inner.get(&record).unwrap().outcome.as_deref(),
        Some("lost")
    );

    // Cancelled by the parent.
    let journal = Probe::new(false);
    let hold = key();
    let mut d = start(&client, job(&hold), &journal).await;
    until_named(&mut d).await;
    let record = d.journal_id().unwrap().to_owned();
    let done = d.cancel().await;
    let label = journal
        .inner
        .get(&record)
        .unwrap()
        .outcome
        .expect("settled");
    assert!(
        matches!(label.as_str(), "canceled" | "cancel_requested"),
        "{label} for {:?}",
        done.outcome
    );

    // Dropped unsettled: the drop-time cancel settles it too.
    let journal = Probe::new(false);
    let hold = key();
    let mut d = start(&client, job(&hold), &journal).await;
    until_named(&mut d).await;
    let record = d.journal_id().unwrap().to_owned();
    drop(d);
    tokio::time::timeout(GUARD, async {
        while journal.inner.get(&record).unwrap().is_open() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the dropped delegation's record was settled");
}

#[tokio::test]
async fn progress_is_written_when_the_child_is_named_and_then_throttled() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    // Sixteen events 100 ms apart: about 1.6 s of stream.
    let d = start(&client, job("tick:go"), &journal).await;
    let record = d.journal_id().unwrap().to_owned();
    let done = d.wait(std::future::pending()).await;
    assert!(matches!(done.outcome, Outcome::Completed(_)));
    let calls = journal.progress_calls.load(Ordering::SeqCst);
    assert!(
        (1..=4).contains(&calls),
        "one write when named, at most one more per two seconds, one at the end: {calls}"
    );
    let r = journal.inner.get(&record).unwrap();
    assert_eq!(r.task_id, done.task_id);
}

#[test]
fn a_delegation_without_a_journal_has_no_record() {
    // `Delegation::start` is unchanged: no journal, no id.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let client = client_for(serve().await);
        let d = Delegation::start(client, job("done")).await.expect("start");
        assert_eq!(d.journal_id(), None);
        let _ = d.wait(std::future::pending()).await;
    });
}
