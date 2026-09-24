// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The gate's fixtures: the executor and store it serves with, the metric
//! reader and webhook it observes through, and the book's catalogue.

use std::collections::BTreeSet;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_sdk::server::store::InMemoryTaskStore;
use a2a_protocol_sdk::server::{EventEmitter, TaskStore, agent_executor};
use a2a_protocol_sdk::types::params::ListTasksParams;
use a2a_protocol_sdk::types::responses::TaskListResponse;
use a2a_protocol_sdk::types::{
    A2aError, A2aResult, Message, MessageRole, MessageSendParams, Part, Task, TaskId, TaskState,
};
use opentelemetry_sdk::metrics::ManualReader;
use opentelemetry_sdk::metrics::data::ResourceMetrics;
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::trace::SpanData;

/// What each executor run saw as its trace context, keyed by the message text
/// (which names the binding the call came in on).
pub static SEEN: Mutex<Vec<(String, Option<String>)>> = Mutex::new(Vec::new());

pub struct Recorder;

agent_executor!(Recorder, |ctx, queue| async {
    let text = ctx.message.text().unwrap_or_default().to_owned();
    let downstream = ctx.trace_context().map(|t| t.span_id().to_owned());
    SEEN.lock().expect("lock").push((text, downstream));
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    emit.artifact("out", vec![Part::text("done")], None, Some(true))
        .await?;
    emit.status(TaskState::Completed).await
});

/// An in-memory store that refuses to persist any non-initial state of a task
/// in the context `persist-fail`, so the persistence-error path runs.
pub struct FailingStore(pub InMemoryTaskStore);

pub type Fut<'a, T> = Pin<Box<dyn std::future::Future<Output = A2aResult<T>> + Send + 'a>>;

impl TaskStore for FailingStore {
    fn save<'a>(&'a self, task: &'a Task) -> Fut<'a, ()> {
        if task.context_id.0 == "persist-fail" && task.status.state != TaskState::Submitted {
            return Box::pin(async { Err(A2aError::internal("injected persistence failure")) });
        }
        self.0.save(task)
    }
    fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
        self.0.get(id)
    }
    fn list<'a>(&'a self, params: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
        self.0.list(params)
    }
    fn insert_if_absent<'a>(&'a self, task: &'a Task) -> Fut<'a, bool> {
        self.0.insert_if_absent(task)
    }
    fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
        self.0.delete(id)
    }
}

/// `ManualReader` is not `Clone`, and the provider takes its reader by value.
#[derive(Clone, Debug)]
pub struct SharedReader(pub Arc<ManualReader>);

impl MetricReader for SharedReader {
    fn register_pipeline(&self, pipeline: std::sync::Weak<opentelemetry_sdk::metrics::Pipeline>) {
        self.0.register_pipeline(pipeline);
    }
    fn collect(&self, rm: &mut ResourceMetrics) -> opentelemetry_sdk::error::OTelSdkResult {
        self.0.collect(rm)
    }
    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        self.0.force_flush()
    }
    fn shutdown_with_timeout(&self, timeout: Duration) -> opentelemetry_sdk::error::OTelSdkResult {
        self.0.shutdown_with_timeout(timeout)
    }
    fn temporality(
        &self,
        kind: opentelemetry_sdk::metrics::InstrumentKind,
    ) -> opentelemetry_sdk::metrics::Temporality {
        self.0.temporality(kind)
    }
}

/// A webhook that accepts every POST, so a push delivery completes, and
/// counts what it received.
pub async fn webhook(hits: Arc<std::sync::atomic::AtomicUsize>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0_u8; 64 * 1024];
                let _ = s.read(&mut buf).await;
                let _ = s
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                    .await;
            });
        }
    });
    format!("http://{addr}/hook")
}

pub fn message(text: &str, context: Option<&str>) -> MessageSendParams {
    let mut msg = Message::new(
        format!("m-{text}"),
        MessageRole::User,
        vec![Part::text(text)],
    );
    msg.context_id = context.map(Into::into);
    MessageSendParams::new(msg)
}

/// The catalogue adopters build dashboards from, read from the page itself.
pub fn book_catalogue() -> BTreeSet<String> {
    let page = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../book/src/deployment/observability.md"
    ))
    .expect("observability page");
    let section = page
        .split("## The catalogue")
        .nth(1)
        .expect("a catalogue section");
    section
        .lines()
        .skip_while(|l| !l.starts_with("| Metric |"))
        .skip(2)
        .take_while(|l| l.starts_with('|'))
        .filter_map(|l| l.split('`').nth(1).map(str::to_owned))
        .collect()
}

pub fn attr(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}
