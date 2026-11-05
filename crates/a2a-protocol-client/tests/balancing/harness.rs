// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Replicas that answer `GetTask` with a task named after themselves, fail
//! or stall on command, and count what they receive; and a layer-4
//! forwarder that sends each new connection to the next replica.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use a2a_protocol_types::{Task, TaskState, TaskStatus};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

/// What a replica does with the next request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Answers.
    Ok,
    /// Answers with this status.
    Status(u16),
    /// Holds the request until the test ends.
    Stall,
}

pub struct Replica {
    pub name: String,
    pub addr: SocketAddr,
    requests: AtomicUsize,
    connections: AtomicUsize,
    mode: Mutex<Mode>,
    /// The `Host` header of every request.
    hosts: Mutex<Vec<String>>,
    /// The JSON-RPC method of every request (or the HTTP method and path).
    methods: Mutex<Vec<String>>,
    stall: Semaphore,
}

impl Replica {
    /// A replica on `ip`, any port.
    pub async fn start_on(name: &str, ip: &str) -> Arc<Self> {
        Self::serve(name, TcpListener::bind((ip, 0)).await.expect("bind"))
    }

    /// A replica serving on `listener`.
    pub fn serve(name: &str, listener: TcpListener) -> Arc<Self> {
        let replica = Arc::new(Self {
            name: name.to_owned(),
            addr: listener.local_addr().expect("addr"),
            requests: AtomicUsize::new(0),
            connections: AtomicUsize::new(0),
            mode: Mutex::new(Mode::Ok),
            hosts: Mutex::new(Vec::new()),
            methods: Mutex::new(Vec::new()),
            stall: Semaphore::new(0),
        });
        let seen = Arc::clone(&replica);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                seen.connections.fetch_add(1, Ordering::SeqCst);
                let r = Arc::clone(&seen);
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req| {
                        let r = Arc::clone(&r);
                        async move { r.answer(req).await }
                    });
                    let _ = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
                });
            }
        });
        replica
    }

    pub async fn start(name: &str) -> Arc<Self> {
        Self::start_on(name, "127.0.0.1").await
    }

    async fn answer(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<Full<Bytes>>, std::convert::Infallible> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let host = req
            .headers()
            .get(hyper::header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .or_else(|| req.uri().authority().map(ToString::to_string))
            .unwrap_or_default();
        self.hosts.lock().unwrap().push(host);
        let rest = req.uri().path().starts_with("/tasks/");
        let path = format!("{} {}", req.method(), req.uri().path());
        let body = req
            .into_body()
            .collect()
            .await
            .map(|b| b.to_bytes())
            .unwrap_or_default();
        let rpc: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        let method = rpc
            .get("method")
            .and_then(serde_json::Value::as_str)
            .map_or(path, str::to_owned);
        self.methods.lock().unwrap().push(method);
        let mode = *self.mode.lock().unwrap();
        match mode {
            Mode::Ok => {}
            Mode::Status(status) => {
                return Ok(hyper::Response::builder()
                    .status(status)
                    .body(Full::new(Bytes::new()))
                    .unwrap());
            }
            Mode::Stall => {
                let _ = self.stall.acquire().await;
            }
        }
        let task = serde_json::to_value(Task::new(
            self.name.as_str(),
            "ctx",
            TaskStatus::new(TaskState::Completed),
        ))
        .unwrap();
        let body = if rest {
            task
        } else {
            serde_json::json!({"jsonrpc": "2.0", "id": rpc.get("id").cloned(), "result": task})
        };
        Ok(hyper::Response::builder()
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body.to_string())))
            .unwrap())
    }

    pub fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn hosts(&self) -> Vec<String> {
        self.hosts.lock().unwrap().clone()
    }

    pub fn methods(&self) -> Vec<String> {
        self.methods.lock().unwrap().clone()
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

/// Three replicas, `a`, `b` and `c`.
pub async fn three() -> [Arc<Replica>; 3] {
    [
        Replica::start("a").await,
        Replica::start("b").await,
        Replica::start("c").await,
    ]
}

pub fn urls(replicas: &[Arc<Replica>]) -> Vec<String> {
    replicas.iter().map(|r| r.url()).collect()
}

pub fn counts(replicas: &[Arc<Replica>]) -> Vec<usize> {
    replicas.iter().map(|r| r.requests()).collect()
}

/// Moves tokio's clock on by `by`, between calls.
pub async fn elapse(by: std::time::Duration) {
    tokio::time::pause();
    tokio::time::advance(by).await;
    tokio::time::resume();
}

/// The task id a `GetTask` was answered with: the replica's name.
pub async fn get(
    client: &a2a_protocol_client::A2aClient,
) -> Result<String, a2a_protocol_client::ClientError> {
    client
        .get_task(a2a_protocol_types::TaskQueryParams::new("t"))
        .await
        .map(|task| task.id.to_string())
}
