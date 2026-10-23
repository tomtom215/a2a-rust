// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A streaming call ends when its stream does (`stream.rs`): its metric is
//! recorded then, with the stream's outcome, and its span's last exit —
//! the end time `tracing-opentelemetry` exports — is then too.

use std::sync::Mutex;
use std::time::Instant;

use a2a_protocol_types::error::A2aError;

use super::*;
use crate::error::ServerError;

/// Method, error type and duration of each recorded call.
type Calls = Arc<Mutex<Vec<(Option<String>, Option<String>, Duration)>>>;

struct Recording(Calls);

impl Metrics for Recording {
    fn on_rpc_call(&self, call: &RpcCall<'_>) {
        self.0.lock().unwrap().push((
            call.method.map(str::to_owned),
            call.error_type.map(str::to_owned),
            call.duration,
        ));
    }
}

fn handler(calls: &Calls) -> RequestHandler {
    struct Idle;
    crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
    crate::RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_metrics(Recording(Arc::clone(calls)))
        .build()
        .unwrap()
}

/// A call that opens a stream, which claims it while it is set up.
async fn open_stream(handler: &RequestHandler) -> Option<StreamEnd> {
    ServerSpan::open(handler, RpcSystem::JsonRpc, "SubscribeToTask", None)
        .run_with(async { Ok::<_, ServerError>(()) }, |_| StreamEnd::claim())
        .await
}

const LIFETIME: Duration = Duration::from_millis(30);

#[tokio::test]
async fn a_streaming_call_is_recorded_when_its_stream_completes() {
    let calls = Calls::default();
    let handler = handler(&calls);
    let end = open_stream(&handler)
        .await
        .expect("the stream claims the call");
    assert!(
        calls.lock().unwrap().is_empty(),
        "recorded when the handler returned, before its stream ended"
    );
    tokio::time::sleep(LIFETIME).await;
    end.end(StreamOutcome::Completed);
    let calls = calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "{calls:?}");
    let (method, error, duration) = &calls[0];
    assert_eq!(
        method.as_deref(),
        Some("lf.a2a.v1.A2AService/SubscribeToTask")
    );
    assert_eq!(*error, None);
    assert!(
        *duration >= LIFETIME,
        "the duration {duration:?} ends at the handshake"
    );
}

#[tokio::test]
async fn a_stream_that_fails_records_its_error() {
    let calls = Calls::default();
    let handler = handler(&calls);
    let end = open_stream(&handler).await.expect("claimed");
    end.end(StreamOutcome::Failed(A2aError::internal("broke")));
    let errors: Vec<_> = calls.lock().unwrap().iter().map(|c| c.1.clone()).collect();
    assert_eq!(errors, [Some("-32603".to_owned())]);
}

#[tokio::test]
async fn a_stream_dropped_unfinished_is_cancelled() {
    let calls = Calls::default();
    let handler = handler(&calls);
    drop(open_stream(&handler).await.expect("claimed"));
    let errors: Vec<_> = calls.lock().unwrap().iter().map(|c| c.1.clone()).collect();
    assert_eq!(errors, [Some("cancelled".to_owned())]);
}

/// The stream can end before the call that opened it has finished: it is
/// then recorded once, when the call does, with the stream's outcome.
#[tokio::test]
async fn a_stream_that_ends_first_is_recorded_once_by_the_call() {
    let calls = Calls::default();
    let handler = handler(&calls);
    ServerSpan::open(&handler, RpcSystem::JsonRpc, "SubscribeToTask", None)
        .run_with(async { Ok::<_, ServerError>(()) }, |_| {
            let end = StreamEnd::claim().expect("claimed");
            end.end(StreamOutcome::Failed(A2aError::task_not_found("t")));
            assert!(StreamEnd::claim().is_none(), "a call is claimed once");
        })
        .await;
    let errors: Vec<_> = calls.lock().unwrap().iter().map(|c| c.1.clone()).collect();
    assert_eq!(errors, [Some("-32001".to_owned())]);
}

/// A call that failed opened no stream: it is recorded with its own error,
/// now, and a claim made anyway changes nothing.
#[tokio::test]
async fn a_failed_call_is_recorded_now_with_its_own_error() {
    let calls = Calls::default();
    let handler = handler(&calls);
    let end = ServerSpan::open(&handler, RpcSystem::JsonRpc, "SubscribeToTask", None)
        .run_with(
            async { Err::<(), _>(ServerError::TaskNotFound("t".into())) },
            |_| StreamEnd::claim(),
        )
        .await;
    assert_eq!(calls.lock().unwrap().len(), 1);
    drop(end);
    let errors: Vec<_> = calls.lock().unwrap().iter().map(|c| c.1.clone()).collect();
    assert_eq!(errors, [Some("-32001".to_owned())]);
}

/// Only a call that can answer with a stream can be claimed; outside a
/// call nothing can.
#[tokio::test]
async fn a_unary_call_cannot_be_claimed() {
    let calls = Calls::default();
    let handler = handler(&calls);
    let claimed = ServerSpan::open(&handler, RpcSystem::JsonRpc, "GetTask", None)
        .run(async { Ok::<_, ServerError>(StreamEnd::claim()) })
        .await
        .unwrap();
    assert!(claimed.is_none());
    assert!(StreamEnd::claim().is_none());
    assert_eq!(calls.lock().unwrap().len(), 1);
}

/// An SSE response is such a stream on JSON-RPC and HTTP+JSON: the call is
/// recorded when the reader ends, with the error its last frame carried.
#[tokio::test]
async fn an_sse_stream_records_its_call_when_its_last_frame_is_sent() {
    use http_body_util::BodyExt as _;
    let calls = Calls::default();
    let handler = handler(&calls);
    let (tx, rx) = tokio::sync::broadcast::channel(8);
    let reader = crate::streaming::InMemoryQueueReader::new(rx);
    let mut response = ServerSpan::open(&handler, RpcSystem::JsonRpc, "SubscribeToTask", None)
        .run_with(async { Ok::<_, ServerError>(reader) }, |out| {
            crate::streaming::build_sse_response(out.unwrap(), None, None, None)
        })
        .await;
    assert!(
        calls.lock().unwrap().is_empty(),
        "recorded at the handshake"
    );
    tx.send(Arc::new(Err(A2aError::stream_lagged(3)))).unwrap();
    drop(tx);
    while let Some(frame) = response.body_mut().frame().await {
        frame.unwrap();
    }
    // The writer records the call as it finishes, after its last frame.
    let deadline = Instant::now() + Duration::from_secs(5);
    while calls.lock().unwrap().is_empty() && Instant::now() < deadline {
        tokio::task::yield_now().await;
    }
    let errors: Vec<_> = calls.lock().unwrap().iter().map(|c| c.1.clone()).collect();
    assert_eq!(errors, [Some("-32603".to_owned())]);
}

/// A client that goes away ends the stream at once — not at the next frame,
/// which a quiet task may not send for a long time — and the call with it,
/// as cancelled.
#[tokio::test]
async fn an_sse_client_that_goes_away_cancels_the_call_at_once() {
    let calls = Calls::default();
    let handler = handler(&calls);
    let (tx, rx) = tokio::sync::broadcast::channel::<crate::streaming::event_queue::Shared>(8);
    let reader = crate::streaming::InMemoryQueueReader::new(rx);
    let response = ServerSpan::open(&handler, RpcSystem::JsonRpc, "SubscribeToTask", None)
        .run_with(async { Ok::<_, ServerError>(reader) }, |out| {
            crate::streaming::build_sse_response(out.unwrap(), None, None, None)
        })
        .await;
    drop(response);
    let deadline = Instant::now() + Duration::from_secs(5);
    while calls.lock().unwrap().is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let errors: Vec<_> = calls.lock().unwrap().iter().map(|c| c.1.clone()).collect();
    assert_eq!(errors, [Some("cancelled".to_owned())]);
    drop(tx);
}

/// The call's span is entered when its stream ends, so its last exit — the
/// end time `tracing-opentelemetry` exports — is the stream's end, not the
/// handshake's.
#[cfg(feature = "tracing")]
#[tokio::test]
async fn the_call_span_is_last_exited_when_its_stream_ends() {
    use tracing_subscriber::layer::SubscriberExt as _;

    #[derive(Clone, Default)]
    struct LastExit(Arc<Mutex<Option<Instant>>>);
    impl<S> tracing_subscriber::Layer<S> for LastExit
    where
        S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    {
        fn on_exit(&self, id: &tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
            if ctx.span(id).is_some_and(|s| s.name() == "a2a.rpc") {
                *self.0.lock().unwrap() = Some(Instant::now());
            }
        }
    }

    let exits = LastExit::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(exits.clone()));
    let calls = Calls::default();
    let handler = handler(&calls);
    let end = open_stream(&handler).await.expect("claimed");
    tokio::time::sleep(LIFETIME).await;
    let stream_ended = Instant::now();
    end.end(StreamOutcome::Completed);
    let last = exits.0.lock().unwrap().expect("the span was entered");
    assert!(
        last >= stream_ended,
        "the span was last exited {:?} before its stream ended",
        stream_ended - last
    );
}
