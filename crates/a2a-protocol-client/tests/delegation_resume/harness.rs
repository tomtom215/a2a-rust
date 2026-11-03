// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The server, the severable proxy and the helpers the cases share.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use a2a_protocol_client::delegation::Delegation;
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::store::TenantAwareInMemoryTaskStore;
use a2a_protocol_server::{Dispatcher, EventEmitter, agent_executor};
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskQueryParams, TaskState};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, watch};

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

struct Worker;
agent_executor!(Worker, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    let text = ctx.message.text().unwrap_or_default().to_owned();
    if text.starts_with("hold:") {
        // Works until the test opens its gate, or is cancelled.
        emit.status(TaskState::Working).await?;
        let released = gate(&text);
        tokio::select! {
            () = released.notified() => emit.status(TaskState::Completed).await,
            () = ctx.cancellation_token.cancelled() => Ok(()),
        }
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

/// A TCP forwarder that can sever every open connection and refuse new ones.
pub struct Proxy {
    pub addr: SocketAddr,
    generation: watch::Sender<u64>,
    blocked: Arc<AtomicBool>,
    accepted: Arc<AtomicUsize>,
}

impl Proxy {
    pub async fn start(upstream: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (generation, _rx) = watch::channel(0_u64);
        let blocked = Arc::new(AtomicBool::new(false));
        let accepted = Arc::new(AtomicUsize::new(0));
        let (g, b, a) = (
            generation.clone(),
            Arc::clone(&blocked),
            Arc::clone(&accepted),
        );
        tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                a.fetch_add(1, Ordering::SeqCst);
                if b.load(Ordering::SeqCst) {
                    drop(inbound);
                    continue;
                }
                let mut cut = g.subscribe();
                tokio::spawn(async move {
                    let Ok(mut outbound) = TcpStream::connect(upstream).await else {
                        return;
                    };
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => {}
                        _ = cut.changed() => {}
                    }
                });
            }
        });
        Self {
            addr,
            generation,
            blocked,
            accepted,
        }
    }

    /// Severs every open connection; new ones are served unless blocked.
    pub fn cut(&self) {
        self.generation.send_modify(|g| *g += 1);
    }

    pub fn block(&self, on: bool) {
        self.blocked.store(on, Ordering::SeqCst);
    }

    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

pub fn client_for(addr: SocketAddr) -> Arc<A2aClient> {
    Arc::new(
        ClientBuilder::new(format!("http://{addr}"))
            .build()
            .expect("client"),
    )
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

pub async fn state_of(client: &A2aClient, id: &str) -> TaskState {
    client
        .get_task(TaskQueryParams::new(id.to_owned()))
        .await
        .expect("get_task")
        .status
        .state
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

pub async fn until(what: &str, mut ok: impl FnMut() -> bool) {
    tokio::time::timeout(GUARD, async {
        while !ok() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never happened: {what}"));
}
