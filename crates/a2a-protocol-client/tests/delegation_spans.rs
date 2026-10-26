// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The calls a delegation makes from work it spawns — a lease renewal, the
//! cancel sent when the handle is dropped — are in the trace of the task
//! that delegated, not traces of their own.

#![cfg(feature = "otel")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_client::delegation::Delegation;
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::store::InMemoryLeaseStore;
use a2a_protocol_server::{EventEmitter, agent_executor};
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskQueryParams, TaskState};
use opentelemetry::trace::{SpanKind, TracerProvider as _};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanExporter};
use tracing_subscriber::layer::SubscriberExt as _;

const GET_TASK: &str = "lf.a2a.v1.A2AService/GetTask";
const CANCEL_TASK: &str = "lf.a2a.v1.A2AService/CancelTask";

#[derive(Clone, Debug, Default)]
struct Keep(Arc<Mutex<Vec<SpanData>>>);

impl SpanExporter for Keep {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        self.0.lock().unwrap().extend(batch);
        std::future::ready(Ok(()))
    }
}

/// Works until cancelled.
struct Sleeper;
agent_executor!(Sleeper, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Working)
        .await?;
    ctx.cancellation_token.cancelled().await;
    Ok(())
});

async fn worker() -> Arc<A2aClient> {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Sleeper)
            .without_agent_card()
            .with_lease_store(InMemoryLeaseStore::new())
            .build()
            .expect("handler"),
    );
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler))
        .await
        .expect("serve");
    Arc::new(
        ClientBuilder::new(format!("http://{addr}"))
            .build()
            .expect("client"),
    )
}

fn job() -> MessageSendParams {
    MessageSendParams::new(Message::user(
        uuid::Uuid::new_v4().to_string(),
        vec![Part::text("sleep")],
    ))
}

async fn named(child: &mut Delegation) -> String {
    tokio::time::timeout(Duration::from_secs(10), async {
        while child.task_id().is_none() {
            child.next_event().await.expect("an event").expect("ok");
        }
    })
    .await
    .expect("the child's id");
    child.task_id().expect("id").to_owned()
}

/// Runs `body` on one thread, inside a span named `parent`, with every span
/// exported; returns the spans and the parent's trace id. `body` is given the
/// store the exporter fills, to wait on.
fn traced<F, Fut>(body: F) -> (Vec<SpanData>, String)
where
    F: FnOnce(Keep) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let keep = Keep::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(keep.clone())
        .build();
    let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("delegation-test"));
    tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let parent = tracing::info_span!("parent");
        runtime.block_on(tracing::Instrument::instrument(body(keep.clone()), parent));
    });
    let spans = std::mem::take(&mut *keep.0.lock().unwrap());
    let parent = spans
        .iter()
        .find(|s| s.name == "parent")
        .expect("the parent span")
        .span_context
        .trace_id()
        .to_string();
    (spans, parent)
}

fn client_spans<'a>(spans: &'a [SpanData], name: &str) -> Vec<&'a SpanData> {
    spans
        .iter()
        .filter(|s| s.span_kind == SpanKind::Client && s.name == name)
        .collect()
}

/// The lease renewals exported so far: each is a `GetTask` client span.
fn recorded_renewals(keep: &Keep) -> usize {
    client_spans(&keep.0.lock().unwrap(), GET_TASK).len()
}

/// The cancel a dropped handle sends runs on a spawned task, after the
/// code that dropped it has moved on; its call is still in the trace.
#[test]
fn the_cancel_a_dropped_handle_sends_is_in_the_parents_trace() {
    let (spans, trace) = traced(|_| async {
        let client = worker().await;
        let mut child = Delegation::start(Arc::clone(&client), job())
            .await
            .expect("start");
        let id = named(&mut child).await;
        drop(child);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let task = client.get_task(TaskQueryParams::new(&id)).await.unwrap();
                if task.status.state == TaskState::Canceled {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the child is cancelled");
    });
    let cancels = client_spans(&spans, CANCEL_TASK);
    assert_eq!(
        cancels.len(),
        1,
        "{:?}",
        spans.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    assert_eq!(cancels[0].span_context.trace_id().to_string(), trace);
}

/// Each lease renewal is a `GetTask` from the renewing task; every one is in
/// the trace of the task that delegated.
#[test]
fn lease_renewals_are_in_the_parents_trace() {
    let ttl = Duration::from_secs(1);
    let (spans, trace) = traced(|keep| async move {
        let client = worker().await;
        let mut child = Delegation::start_leased(Arc::clone(&client), job(), ttl)
            .await
            .expect("start");
        named(&mut child).await;
        // A renewal is due every third of the ttl, so the second comes about
        // two thirds of the way in. Wait for it, not for a fixed time, so a
        // slow moment on a busy machine delays this test and does not fail it;
        // if it never comes, fail after 10 s and say what was recorded.
        let both = tokio::time::timeout(Duration::from_secs(10), async {
            while recorded_renewals(&keep) < 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        let _ = child.cancel().await;
        // Read before asserting: a panic holding the lock would poison it for
        // the exporter, which locks it on every span it receives.
        let names: Vec<_> = keep
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.name.to_string())
            .collect();
        assert!(
            both.is_ok(),
            "fewer than 2 renewals in 10 s; spans: {names:?}"
        );
    });
    let renewals = client_spans(&spans, GET_TASK);
    assert!(renewals.len() >= 2, "{} renewals", renewals.len());
    for renewal in renewals {
        assert_eq!(renewal.span_context.trace_id().to_string(), trace);
    }
}
