// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The server, the probing journal and the helpers the cases share.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use a2a_protocol_client::delegation::Delegation;
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

pub const GUARD: Duration = Duration::from_secs(20);

/// What a `hold:` child waits for: one gate per message text.
static GATES: LazyLock<Mutex<HashMap<String, Arc<Notify>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn gate(key: &str) -> Arc<Notify> {
    Arc::clone(
        GATES
            .lock()
            .unwrap()
            .entry(key.to_owned())
            .or_insert_with(|| Arc::new(Notify::new())),
    )
}

/// Orders the journal's `begin` against the executor's start.
pub static CLOCK: AtomicUsize = AtomicUsize::new(1);
pub static STARTS: LazyLock<Mutex<HashMap<String, usize>>> =
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
    } else if text.starts_with("tick:") || text.starts_with("slowtick:") {
        // A stream of events 100 ms apart, then done: sixteen artifact chunks,
        // or twenty-six (about 2.6 s, longer than the journal's two seconds)
        // for `slowtick:`.
        let lines = if text.starts_with("slow") { 26 } else { 16 };
        emit.status(TaskState::Working).await?;
        for i in 0..lines {
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

pub async fn serve() -> SocketAddr {
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

pub fn client_for(addr: SocketAddr) -> Arc<A2aClient> {
    Arc::new(
        ClientBuilder::new(format!("http://{addr}"))
            .build()
            .expect("client"),
    )
}

/// A client that knows its agent honours idempotency keys, as one built from a
/// card that declares the extension does.
pub fn honouring_client_for(addr: SocketAddr) -> Arc<A2aClient> {
    Arc::new(
        ClientBuilder::new(format!("http://{addr}"))
            .with_peer_honouring_idempotency(true)
            .build()
            .expect("client"),
    )
}

/// Unix milliseconds now, as the journal's clock reads them.
pub fn unix_ms() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch");
    i64::try_from(since.as_millis()).expect("fits")
}

pub fn job(text: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user(
        uuid::Uuid::new_v4().to_string(),
        vec![Part::text(text)],
    ))
}

pub fn key() -> String {
    format!("hold:{}", uuid::Uuid::new_v4())
}

/// Reads events until the child is named.
pub async fn until_named(d: &mut Delegation) -> String {
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
pub struct Probe {
    pub inner: InMemoryDelegationJournal,
    pub began_at: AtomicUsize,
    pub progress_calls: AtomicUsize,
    /// When each `progress` call arrived.
    pub progress_at: Mutex<Vec<Instant>>,
    /// Each `settle` call: the record id and the label.
    pub settled: Mutex<Vec<(String, String)>>,
    pub refuse_begin: bool,
}

impl Probe {
    pub fn new(refuse_begin: bool) -> Arc<Self> {
        Arc::new(Self {
            inner: InMemoryDelegationJournal::new(),
            began_at: AtomicUsize::new(0),
            progress_calls: AtomicUsize::new(0),
            progress_at: Mutex::new(Vec::new()),
            settled: Mutex::new(Vec::new()),
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
        self.progress_at.lock().unwrap().push(Instant::now());
        self.inner.progress(id, task_id, last_event_id, now_ms)
    }

    fn settle<'a>(&'a self, id: &'a str, outcome: &'a str, now_ms: i64) -> Fut<'a, ()> {
        self.settled
            .lock()
            .unwrap()
            .push((id.to_owned(), outcome.to_owned()));
        self.inner.settle(id, outcome, now_ms)
    }

    fn open<'a>(&'a self, owner: &'a str) -> Fut<'a, Vec<DelegationRecord>> {
        self.inner.open(owner)
    }

    fn purge_settled(&self, before: i64) -> Fut<'_, u64> {
        self.inner.purge_settled(before)
    }
}

pub async fn start(
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

pub fn keyed(text: &str, key: &str) -> MessageSendParams {
    let mut params = job(text);
    idempotency::set_key(&mut params.message, key).expect("a valid key");
    params
}
