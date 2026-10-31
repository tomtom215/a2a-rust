// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A delegation resumes a stream the network cut before it gives up on the
//! child, and a supervisor can pick a child up again (audit H4, ADR 0030).
//!
//! The server is this repository's, over real sockets. A proxy sits in front
//! of it that can sever every open connection (`cut`) and refuse new ones
//! (`block`), which is what an L4 reset or a rollout does to a long-lived
//! SSE stream. Each test asserts what became of the child on the server, not
//! only what the delegation reported.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use a2a_protocol_client::delegation::{Delegation, Outcome, ResumePolicy};
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::store::TenantAwareInMemoryTaskStore;
use a2a_protocol_server::{Dispatcher, EventEmitter, agent_executor};
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskQueryParams, TaskState};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, watch};

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

/// A TCP forwarder that can sever every open connection and refuse new ones.
struct Proxy {
    addr: SocketAddr,
    generation: watch::Sender<u64>,
    blocked: Arc<AtomicBool>,
    accepted: Arc<AtomicUsize>,
}

impl Proxy {
    async fn start(upstream: SocketAddr) -> Self {
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
    fn cut(&self) {
        self.generation.send_modify(|g| *g += 1);
    }

    fn block(&self, on: bool) {
        self.blocked.store(on, Ordering::SeqCst);
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
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

async fn state_of(client: &A2aClient, id: &str) -> TaskState {
    client
        .get_task(TaskQueryParams {
            tenant: None,
            id: id.to_owned(),
            history_length: None,
        })
        .await
        .expect("get_task")
        .status
        .state
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

async fn until(what: &str, mut ok: impl FnMut() -> bool) {
    tokio::time::timeout(GUARD, async {
        while !ok() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never happened: {what}"));
}

#[tokio::test]
async fn a_cut_stream_is_resumed_and_the_running_child_completes() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let hold = key();

    let mut d = Delegation::start(via, job(&hold)).await.expect("start");
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));

    let before = proxy.accepted();
    proxy.cut();
    // The delegation comes back through the proxy while the child still runs.
    until("the delegation reconnected", || proxy.accepted() > before).await;
    assert_eq!(state_of(&direct, &id).await, TaskState::Working);
    gate(&hold).notify_one();

    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
    assert_eq!(
        state_of(&direct, &id).await,
        TaskState::Completed,
        "the child was never cancelled"
    );
}

#[tokio::test]
async fn a_child_that_finished_while_the_stream_was_down_is_reported_as_finished() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let hold = key();

    let mut d = Delegation::start(via, job(&hold)).await.expect("start");
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));

    // Down: the child finishes where nobody can see it, then the link returns.
    proxy.block(true);
    proxy.cut();
    gate(&hold).notify_one();
    tokio::time::timeout(GUARD, async {
        while state_of(&direct, &id).await != TaskState::Completed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the child finished");
    proxy.block(false);

    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    assert!(
        matches!(done.outcome, Outcome::Completed(_)),
        "a finished child is not Lost: {done:?}"
    );
    assert_eq!(state_of(&direct, &id).await, TaskState::Completed);
}

#[tokio::test]
async fn a_link_that_stays_down_is_given_up_on_after_the_policy_and_no_longer() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let hold = key();
    let policy = ResumePolicy::none()
        .with_attempts(3)
        .with_backoff(Duration::from_millis(100), Duration::from_millis(100))
        .with_deadline(Duration::from_secs(5));

    let mut d = Delegation::start(via, job(&hold))
        .await
        .expect("start")
        .with_resume(policy);
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));

    proxy.block(true);
    let before = proxy.accepted();
    let cut_at = std::time::Instant::now();
    proxy.cut();
    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    assert!(matches!(done.outcome, Outcome::Lost(_)), "{done:?}");
    // Three attempts wait before the second and the third: 2 x 100 ms. A
    // lower bound only; nothing about how fast a refused connection fails.
    assert!(
        cut_at.elapsed() >= Duration::from_millis(190),
        "the attempts were spaced by the backoff: {:?}",
        cut_at.elapsed()
    );
    // Three reopenings, a look at the task after each, and the cancel: a
    // handful of connections, not a loop.
    let tried = proxy.accepted() - before;
    assert!((3..=40).contains(&tried), "{tried} connections");
    // The link was down for the cancel too, so the child is still running.
    assert_eq!(state_of(&direct, &id).await, TaskState::Working);
    gate(&hold).notify_one();
}

#[tokio::test]
async fn the_deadline_ends_a_loss_that_the_attempts_alone_would_keep_working_on() {
    // Ten thousand attempts, 20 ms apart: only the 400 ms deadline stops this.
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let hold = key();
    let policy = ResumePolicy::none()
        .with_attempts(10_000)
        .with_backoff(Duration::from_millis(20), Duration::from_millis(20))
        .with_deadline(Duration::from_millis(400));

    let mut d = Delegation::start(via, job(&hold))
        .await
        .expect("start")
        .with_resume(policy);
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));

    proxy.block(true);
    let before = proxy.accepted();
    let cut_at = std::time::Instant::now();
    proxy.cut();
    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("the deadline ended it")
        .expect("joined");
    let took = cut_at.elapsed();
    assert!(matches!(done.outcome, Outcome::Lost(_)), "{done:?}");
    assert!(
        took >= Duration::from_millis(300),
        "it worked on the loss: {took:?}"
    );
    assert!(
        took < Duration::from_secs(10),
        "and stopped at the deadline: {took:?}"
    );
    let tried = proxy.accepted() - before;
    assert!(
        tried < 500,
        "{tried} connections, not ten thousand attempts"
    );
    assert_eq!(state_of(&direct, &id).await, TaskState::Working);
    gate(&hold).notify_one();
}

#[tokio::test]
async fn a_link_that_keeps_delivering_is_resumed_past_the_attempt_limit() {
    // Two attempts in a row would be spent by three cuts. But the position
    // moves between the cuts, so each one starts the count again.
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let policy = ResumePolicy::default()
        .with_attempts(2)
        .with_backoff(Duration::from_millis(10), Duration::from_millis(50));

    let mut d = Delegation::start(via, job("tick:go"))
        .await
        .expect("start")
        .with_resume(policy);
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(350)).await;
        proxy.cut();
    }
    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
    assert_eq!(state_of(&direct, &id).await, TaskState::Completed);
}

/// `last_event_id` is the position a resume or a reattach starts from, so it
/// has to be the stream's: absent before the server has numbered an event, then
/// one of its numbers, and larger after more events.
#[tokio::test]
async fn the_recorded_position_is_the_streams_and_it_advances() {
    let upstream = serve().await;
    let mut d = Delegation::start(client_for(upstream), job("tick:go"))
        .await
        .expect("start");
    assert_eq!(d.last_event_id(), None, "nothing received yet");
    let number = |d: &Delegation| d.last_event_id().and_then(|p| p.parse::<u64>().ok());
    let first = tokio::time::timeout(GUARD, async {
        loop {
            d.next_event()
                .await
                .expect("an event")
                .expect("not an error");
            if let Some(n) = number(&d) {
                return n;
            }
        }
    })
    .await
    .expect("the server numbered an event");
    let later = tokio::time::timeout(GUARD, async {
        loop {
            d.next_event()
                .await
                .expect("an event")
                .expect("not an error");
            if let Some(n) = number(&d).filter(|n| *n > first) {
                return n;
            }
        }
    })
    .await
    .expect("and numbered a later one");
    assert!(later > first, "{first} then {later}");
    let _ = d.detach();
}

#[tokio::test]
async fn a_supervisor_that_restarted_reattaches_to_its_running_child() {
    let upstream = serve().await;
    let hold = key();

    let mut first = Delegation::start(client_for(upstream), job(&hold))
        .await
        .expect("start");
    let id = until_named(&mut first).await;
    let position = first.last_event_id().map(str::to_owned);
    assert_eq!(first.detach().as_deref(), Some(id.as_str()));

    // A new process: a new client, the recorded id and position.
    let again = Delegation::reattach(client_for(upstream), None, id.clone(), position)
        .await
        .expect("reattached");
    assert!(!again.is_settled());
    assert_eq!(again.task_id(), Some(id.as_str()));
    gate(&hold).notify_one();
    let done = tokio::time::timeout(GUARD, again.wait(std::future::pending()))
        .await
        .expect("bounded");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
    assert_eq!(
        state_of(&client_for(upstream), &id).await,
        TaskState::Completed
    );
}

#[tokio::test]
async fn reattaching_to_a_child_that_has_settled_returns_it_settled() {
    let upstream = serve().await;
    let client = client_for(upstream);
    let done = Delegation::start(Arc::clone(&client), job("done"))
        .await
        .expect("start")
        .wait(std::future::pending())
        .await;
    assert!(matches!(done.outcome, Outcome::Completed(_)));
    let id = done.task_id.expect("named");

    let again = Delegation::reattach(Arc::clone(&client), None, id.clone(), None)
        .await
        .expect("reattached to a settled child");
    let settled = tokio::time::timeout(GUARD, again.wait(std::future::pending()))
        .await
        .expect("bounded");
    assert!(
        matches!(settled.outcome, Outcome::Completed(_)),
        "{settled:?}"
    );
    assert_eq!(state_of(&client, &id).await, TaskState::Completed);
}

#[tokio::test]
async fn reattaching_to_a_child_that_does_not_exist_fails_and_cancels_nothing() {
    let upstream = serve().await;
    let client = client_for(upstream);
    let err = Delegation::reattach(client, None, "no-such-task", None).await;
    assert!(err.is_err());
}
