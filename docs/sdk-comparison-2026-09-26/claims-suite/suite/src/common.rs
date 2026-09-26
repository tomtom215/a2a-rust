// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Shared helpers for the a2a-rust 0.14.0 claims suite.
//! Depends only on crates.io releases (=0.14.0).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{CallContext, CallOutcome, EventQueueWriter, Metrics, ServerInterceptor};

// ── Ports: only 7500-7999 are allowed. Each test binary picks a base. ────────
static CURSOR: AtomicU16 = AtomicU16::new(0);

/// Returns a free port in [base, base+span). Ports are probed by binding.
pub fn port_in(base: u16, span: u16) -> u16 {
    assert!(base >= 7500 && base + span <= 8000, "port range violation");
    for _ in 0..span {
        let off = CURSOR.fetch_add(1, Ordering::SeqCst) % span;
        let p = base + off;
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
    panic!("no free port in {base}..{}", base + span);
}

// ── A controllable agent ─────────────────────────────────────────────────────
#[derive(Default)]
pub struct Probe {
    pub executions: AtomicUsize,
    pub cancels_observed: AtomicUsize,
    pub finished: AtomicUsize,
    pub request_ids: Mutex<Vec<Option<String>>>,
    pub tenants: Mutex<Vec<Option<String>>>,
}

#[derive(Clone)]
pub struct CtlAgent(pub Arc<Probe>);

impl CtlAgent {
    pub fn new() -> (Self, Arc<Probe>) {
        let p = Arc::new(Probe::default());
        (Self(p.clone()), p)
    }
}

impl AgentExecutor for CtlAgent {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.0.executions.fetch_add(1, Ordering::SeqCst);
            self.0
                .request_ids
                .lock()
                .unwrap()
                .push(ctx.request_id().map(str::to_owned));
            self.0
                .tenants
                .lock()
                .unwrap()
                .push(ctx.tenant().map(str::to_owned));
            let emit = EventEmitter::new(ctx, queue);
            let text = ctx.message.text().unwrap_or("world").to_owned();
            emit.status(TaskState::Working).await?;
            if let Some(ms) = text.strip_prefix("sleep:") {
                let ms: u64 = ms.parse().unwrap_or(1000);
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(ms)) => {}
                    () = ctx.cancellation_token.cancelled() => {
                        self.0.cancels_observed.fetch_add(1, Ordering::SeqCst);
                        return Ok(());
                    }
                }
            } else if let Some(ms) = text.strip_prefix("hang:") {
                let ms: u64 = ms.parse().unwrap_or(1000);
                tokio::time::sleep(Duration::from_millis(ms)).await; // ignores cancellation
            } else if text == "fail" {
                return Err(A2aError::internal("deliberate failure"));
            } else if let Some(rest) = text.strip_prefix("chunks:") {
                let mut it = rest.split(':');
                let n: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(3);
                let ms: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(100);
                for i in 0..n {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    emit.artifact(
                        &format!("chunk-{i}"),
                        vec![Part::text(format!("c{i}"))],
                        None,
                        Some(true),
                    )
                    .await?;
                }
                emit.status(TaskState::Completed).await?;
                self.0.finished.fetch_add(1, Ordering::SeqCst);
                return Ok(());
            }
            emit.artifact("greeting", vec![Part::text(format!("Hello, {text}!"))], None, Some(true))
                .await?;
            emit.status(TaskState::Completed).await?;
            self.0.finished.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

// ── Metrics recorder ─────────────────────────────────────────────────────────
#[derive(Default)]
pub struct MetricsRec {
    pub calls: Mutex<HashMap<String, usize>>,
    pub push: Mutex<Vec<String>>,
}
#[derive(Clone, Default)]
pub struct RecMetrics(pub Arc<MetricsRec>);
impl RecMetrics {
    fn bump(&self, k: &str) {
        *self.0.calls.lock().unwrap().entry(k.to_owned()).or_default() += 1;
    }
    pub fn count(&self, k: &str) -> usize {
        self.0.calls.lock().unwrap().get(k).copied().unwrap_or(0)
    }
    pub fn snapshot(&self) -> HashMap<String, usize> {
        self.0.calls.lock().unwrap().clone()
    }
}
impl Metrics for RecMetrics {
    fn on_request(&self, m: &str) {
        self.bump("on_request");
        self.bump(&format!("on_request:{m}"));
    }
    fn on_response(&self, _m: &str) {
        self.bump("on_response");
    }
    fn on_error(&self, _m: &str, _k: &str) {
        self.bump("on_error");
    }
    fn on_latency(&self, _m: &str, _d: Duration) {
        self.bump("on_latency");
    }
    fn on_queue_depth_change(&self, _n: usize) {
        self.bump("on_queue_depth_change");
    }
    fn on_connection_pool_stats(&self, _s: &a2a_protocol_sdk::server::ConnectionPoolStats) {
        self.bump("on_connection_pool_stats");
    }
    fn on_persistence_error(&self, _o: &str, _k: &str) {
        self.bump("on_persistence_error");
    }
    fn on_push_delivery(&self, outcome: &str) {
        self.bump("on_push_delivery");
        self.0.push.lock().unwrap().push(outcome.to_owned());
    }
    fn on_rpc_call(&self, _c: &a2a_protocol_sdk::server::RpcCall<'_>) {
        self.bump("on_rpc_call");
    }
}

// ── Interceptor recorder ─────────────────────────────────────────────────────
#[derive(Default)]
pub struct IcRec {
    pub events: Mutex<Vec<String>>,
}
#[derive(Clone, Default)]
pub struct RecInterceptor(pub Arc<IcRec>);
impl RecInterceptor {
    pub fn events(&self) -> Vec<String> {
        self.0.events.lock().unwrap().clone()
    }
}
impl ServerInterceptor for RecInterceptor {
    fn before<'a>(&'a self, ctx: &'a CallContext) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.0.events.lock().unwrap().push(format!(
                "before:{}:rid={:?}",
                ctx.method(),
                ctx.request_id()
            ));
            Ok(())
        })
    }
    fn after<'a>(&'a self, ctx: &'a CallContext) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.0.events.lock().unwrap().push(format!("after:{}", ctx.method()));
            Ok(())
        })
    }
    fn on_complete<'a>(
        &'a self,
        ctx: &'a CallContext,
        outcome: CallOutcome<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        let o = match outcome {
            CallOutcome::Succeeded => "Succeeded".to_owned(),
            CallOutcome::Failed(_) => "Failed".to_owned(),
            CallOutcome::Cancelled => "Cancelled".to_owned(),
            _ => "Other".to_owned(),
        };
        Box::pin(async move {
            self.0
                .events
                .lock()
                .unwrap()
                .push(format!("complete:{}:{o}", ctx.method()));
        })
    }
}

pub fn msg(id: &str, text: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user_text(id, text))
}

pub fn uid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static N: AtomicUsize = AtomicUsize::new(0);
    format!(
        "m-{}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        N.fetch_add(1, Ordering::SeqCst)
    )
}

pub fn task_of(r: SendMessageResponse) -> Task {
    match r {
        SendMessageResponse::Task(t) => t,
        other => panic!("expected Task, got {other:?}"),
    }
}

/// Raw JSON-RPC POST with reqwest; returns (status, headers, body).
pub async fn jsonrpc_raw(
    url: &str,
    method: &str,
    params: serde_json::Value,
    headers: &[(&str, &str)],
) -> (u16, reqwest::header::HeaderMap, String) {
    let c = reqwest::Client::new();
    let mut rb = c
        .post(url)
        .header("content-type", "application/json")
        .header("A2A-Version", "1.0")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}));
    for (k, v) in headers {
        rb = rb.header(*k, *v);
    }
    let r = rb.send().await.expect("send");
    let s = r.status().as_u16();
    let h = r.headers().clone();
    let b = r.text().await.unwrap_or_default();
    (s, h, b)
}

pub fn send_params_json(text: &str) -> serde_json::Value {
    serde_json::json!({"message":{"messageId": uid(), "role":"ROLE_USER","parts":[{"text":text}]}})
}
