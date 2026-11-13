// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The readiness-bar harness's executors, measurements and request helpers.

use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_server::RequestHandler;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor_helpers::EventEmitter;
use a2a_protocol_server::serve::{ServeConfig, Server};
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::task::TaskState;
use http_body_util::Full;
use hyper::body::Bytes;

/// The bar's L1 concurrency.
pub const L1_CLIENTS: usize = 8;
/// The store's ceiling: tasks accumulate by protocol design, so an unbounded
/// store's growth is indistinguishable from a leak (the soak test's reasoning).
pub const STORE_CAPACITY: usize = 2_000;

/// An executor that writes one artifact and completes, as L1 says.
pub struct OneArtifact;
impl a2a_protocol_server::executor::AgentExecutor for OneArtifact {
    fn execute<'a>(
        &'a self,
        ctx: &'a a2a_protocol_server::request_context::RequestContext,
        queue: &'a dyn a2a_protocol_server::streaming::EventQueueWriter,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>,
    > {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.artifact(
                "out",
                vec![a2a_protocol_types::Part::text("bar")],
                Some(true),
                Some(true),
            )
            .await?;
            emit.status(TaskState::Completed).await
        })
    }
}

/// An executor that holds its slot for a while, so L4 can overload it.
pub struct Slow;
impl a2a_protocol_server::executor::AgentExecutor for Slow {
    fn execute<'a>(
        &'a self,
        ctx: &'a a2a_protocol_server::request_context::RequestContext,
        queue: &'a dyn a2a_protocol_server::streaming::EventQueueWriter,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>,
    > {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            EventEmitter::new(ctx, queue)
                .status(TaskState::Completed)
                .await
        })
    }
}

pub fn resident_bytes() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").expect("needs /proc");
    let pages: u64 = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    pages * 4096
}

pub fn open_fds() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .expect("needs /proc")
        .count()
}

pub fn alive_tasks() -> usize {
    tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks()
}

pub fn body(n: u64) -> String {
    serde_json::json!({
        "jsonrpc": "2.0", "method": "SendMessage", "id": format!("r-{n}"),
        "params": {"message": {"messageId": format!("m-{n}"), "role": "ROLE_USER", "parts": [{"text": "bar"}]}}
    })
    .to_string()
}

pub fn percentile(samples: &mut [u64], p: f64) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    samples.sort_unstable();
    let idx = ((samples.len() as f64) * p) as usize;
    samples[idx.min(samples.len() - 1)]
}

pub fn client() -> hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::HttpConnector,
    Full<Bytes>,
> {
    hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>()
}

pub fn request(addr: std::net::SocketAddr, n: u64) -> hyper::Request<Full<Bytes>> {
    hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .body(Full::new(Bytes::from(body(n))))
        .unwrap()
}

pub async fn serve(
    handler: Arc<RequestHandler>,
) -> (std::net::SocketAddr, tokio::sync::oneshot::Sender<()>) {
    let server = Server::bind("127.0.0.1:0")
        .await
        .unwrap()
        .with_config(ServeConfig::new().with_max_connections(256));
    let addr = server.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(
        server.serve_with_shutdown(JsonRpcDispatcher::new(handler), async move {
            stopped.await.ok();
        }),
    );
    (addr, stop)
}

/// Waits up to `within` for `ok` to hold; returns the last reading.
pub async fn settles<T: PartialOrd + Copy + std::fmt::Debug>(
    within: Duration,
    read: impl Fn() -> T,
    ok: impl Fn(T) -> bool,
) -> T {
    let deadline = Instant::now() + within;
    loop {
        let v = read();
        if ok(v) || Instant::now() >= deadline {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub struct Rows(pub Vec<(&'static str, bool, String)>);
impl Rows {
    pub fn row(&mut self, id: &'static str, pass: bool, said: String) {
        println!("{id}: {} -- {said}", if pass { "MET" } else { "UNMET" });
        self.0.push((id, pass, said));
    }
}

/// The non-terminal tasks, for the report: id, state and the time of the status.
pub async fn stuck(handler: &RequestHandler) -> Vec<String> {
    let mut out = Vec::new();
    for state in [
        TaskState::Submitted,
        TaskState::Working,
        TaskState::InputRequired,
        TaskState::AuthRequired,
    ] {
        let mut params = ListTasksParams::default();
        params.status = Some(state);
        params.page_size = Some(10);
        for t in handler.on_list_tasks(params, None).await.unwrap().tasks {
            let message = t
                .history
                .as_ref()
                .and_then(|h| h.first())
                .map(|m| m.id.0.clone());
            out.push(format!(
                "{} {:?} at {:?}, first message {:?}, {} history entries, {} artifacts",
                t.id.0,
                t.status.state,
                t.status.timestamp,
                message,
                t.history.as_ref().map_or(0, Vec::len),
                t.artifacts.as_ref().map_or(0, Vec::len),
            ));
        }
    }
    out
}

pub async fn non_terminal(handler: &RequestHandler) -> u64 {
    let mut total = 0;
    for state in [
        TaskState::Submitted,
        TaskState::Working,
        TaskState::InputRequired,
        TaskState::AuthRequired,
    ] {
        let mut params = ListTasksParams::default();
        params.status = Some(state);
        params.page_size = Some(1);
        let page = handler.on_list_tasks(params, None).await.unwrap();
        total += u64::from(page.total_size);
    }
    total
}
