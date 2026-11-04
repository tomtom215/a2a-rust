// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The server, the tap in front of it, and the helpers the cases share.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use a2a_protocol_client::{A2aClient, ClientBuilder, HttpPool};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::store::TenantAwareInMemoryTaskStore;
use a2a_protocol_server::{Dispatcher, EventEmitter, agent_executor};
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

/// A `hold:` child works until the test opens its gate or it is cancelled.
pub struct Gate {
    pub open: Notify,
    pub started: AtomicBool,
}

static GATES: LazyLock<Mutex<HashMap<String, Arc<Gate>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn gate(key: &str) -> Arc<Gate> {
    Arc::clone(
        GATES
            .lock()
            .unwrap()
            .entry(key.to_owned())
            .or_insert_with(|| {
                Arc::new(Gate {
                    open: Notify::new(),
                    started: AtomicBool::new(false),
                })
            }),
    )
}

struct Worker;
agent_executor!(Worker, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    let text = ctx.message.text().unwrap_or_default().to_owned();
    emit.status(TaskState::Working).await?;
    if text.starts_with("hold:") {
        let gate = gate(&text);
        gate.started.store(true, Ordering::SeqCst);
        tokio::select! {
            () = gate.open.notified() => emit.status(TaskState::Completed).await,
            () = ctx.cancellation_token.cancelled() => Ok(()),
        }
    } else {
        emit.status(TaskState::Completed).await
    }
});

/// This repository's JSON-RPC server on a loopback port. It speaks HTTP/1.1
/// and, when the client opens with the HTTP/2 preface, HTTP/2.
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

/// A TCP forwarder that counts what passes through it: connections accepted,
/// HTTP/2 PINGs the client sent (frame type 6, ACK clear), connections the
/// client closed. While `blackhole` is set what the server sends is not
/// passed on, so a PING goes unanswered the way it does when a peer vanishes.
pub struct Tap {
    pub addr: SocketAddr,
    accepted: AtomicUsize,
    pings: AtomicUsize,
    closed: AtomicUsize,
    blackhole: AtomicBool,
}

impl Tap {
    pub async fn start(upstream: SocketAddr) -> Arc<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let tap = Arc::new(Self {
            addr: listener.local_addr().expect("addr"),
            accepted: AtomicUsize::new(0),
            pings: AtomicUsize::new(0),
            closed: AtomicUsize::new(0),
            blackhole: AtomicBool::new(false),
        });
        let seen = Arc::clone(&tap);
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                seen.accepted.fetch_add(1, Ordering::SeqCst);
                let Ok(server) = TcpStream::connect(upstream).await else {
                    continue;
                };
                let (mut client_read, mut client_write) = client.into_split();
                let (mut server_read, mut server_write) = server.into_split();
                let up = Arc::clone(&seen);
                tokio::spawn(async move {
                    let mut frames = Frames::default();
                    let mut chunk = [0_u8; 16_384];
                    loop {
                        match client_read.read(&mut chunk).await {
                            Ok(0) | Err(_) => {
                                up.closed.fetch_add(1, Ordering::SeqCst);
                                return;
                            }
                            Ok(n) => {
                                up.pings
                                    .fetch_add(frames.pings_in(&chunk[..n]), Ordering::SeqCst);
                                if server_write.write_all(&chunk[..n]).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                });
                let down = Arc::clone(&seen);
                tokio::spawn(async move {
                    let mut chunk = [0_u8; 16_384];
                    while let Ok(n) = server_read.read(&mut chunk).await {
                        if n == 0 {
                            return;
                        }
                        if !down.blackhole.load(Ordering::SeqCst)
                            && client_write.write_all(&chunk[..n]).await.is_err()
                        {
                            return;
                        }
                    }
                });
            }
        });
        tap
    }

    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    pub fn pings(&self) -> usize {
        self.pings.load(Ordering::SeqCst)
    }

    pub fn closed(&self) -> usize {
        self.closed.load(Ordering::SeqCst)
    }

    pub fn blackhole(&self, on: bool) {
        self.blackhole.store(on, Ordering::SeqCst);
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

/// Reads the client's byte stream as HTTP/2 frames and counts the PINGs.
#[derive(Default)]
struct Frames {
    buf: Vec<u8>,
    started: bool,
    /// Whether the stream opened with the HTTP/2 preface; an HTTP/1.1
    /// connection has no frames to count.
    h2: bool,
}

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

impl Frames {
    fn pings_in(&mut self, bytes: &[u8]) -> usize {
        self.buf.extend_from_slice(bytes);
        if !self.started {
            if self.buf.len() < PREFACE.len() {
                return 0;
            }
            self.started = true;
            self.h2 = self.buf.starts_with(PREFACE);
            if self.h2 {
                self.buf.drain(..PREFACE.len());
            }
        }
        if !self.h2 {
            self.buf.clear();
            return 0;
        }
        let mut pings = 0;
        while self.buf.len() >= 9 {
            let len = (usize::from(self.buf[0]) << 16)
                | (usize::from(self.buf[1]) << 8)
                | usize::from(self.buf[2]);
            if self.buf.len() < 9 + len {
                break;
            }
            let (kind, flags) = (self.buf[3], self.buf[4]);
            if kind == 6 && flags & 1 == 0 {
                pings += 1;
            }
            self.buf.drain(..9 + len);
        }
        pings
    }
}

/// A client for `url` that sends through `pool`, or through a pool of its own.
pub fn client(url: &str, pool: Option<&HttpPool>) -> A2aClient {
    let builder = ClientBuilder::new(url).with_send_timeout(Duration::from_secs(3600));
    match pool {
        Some(pool) => builder.with_pool(pool),
        None => builder,
    }
    .build()
    .expect("client")
}

pub fn job(text: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user(
        uuid::Uuid::new_v4().to_string(),
        vec![Part::text(text)],
    ))
}

/// One successful call that ends at once.
pub async fn call(client: &A2aClient) {
    client.send_message(job("hi")).await.expect("the call");
}

/// Polls `condition` until it holds, and fails with `what` if it does not
/// within ten seconds.
pub async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Moves the clock on by `by` with sockets open, as `card_freshness` does.
pub async fn elapse(by: Duration) {
    tokio::time::pause();
    tokio::time::advance(by).await;
    tokio::time::resume();
}
