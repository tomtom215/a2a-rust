// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `rpc.client.call.duration` from real calls to this repository's server,
//! read through a `ManualReader` as the server's own metric tests read
//! `rpc.server.call.duration`.

use std::sync::Arc;
use std::time::Duration;

use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

use a2a_protocol_server::{JsonRpcDispatcher, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskQueryParams};

use super::CallMetric;
use crate::{A2aClient, ClientBuilder};

/// A `ManualReader` the provider and the test can both hold.
#[derive(Debug)]
struct SharedReader(Arc<ManualReader>);

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

/// One data point: its sorted attributes, its count and its sum.
type Point = (Vec<(String, String)>, u64, f64);

/// The `rpc.client.call.duration` points `reader` holds, checking the unit
/// and the advisory buckets on the way.
fn points(reader: &ManualReader) -> Vec<Point> {
    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    let Some(metric) = collected
        .scope_metrics()
        .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
        .find(|m| m.name() == "rpc.client.call.duration")
    else {
        return Vec::new();
    };
    assert_eq!(metric.unit(), "s");
    let AggregatedMetrics::F64(MetricData::Histogram(hist)) = metric.data() else {
        panic!("not an f64 histogram");
    };
    hist.data_points()
        .map(|p| {
            assert_eq!(p.bounds().collect::<Vec<_>>(), super::BUCKETS);
            let mut attrs: Vec<(String, String)> = p
                .attributes()
                .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                .collect();
            attrs.sort();
            (attrs, p.count(), p.sum())
        })
        .collect()
}

struct Done;
agent_executor!(Done, |ctx, queue| async {
    a2a_protocol_server::EventEmitter::new(ctx, queue)
        .status(a2a_protocol_types::TaskState::Completed)
        .await
});

/// Says it is working and then never finishes, so its stream ends only when
/// the caller drops it.
struct Endless;
agent_executor!(Endless, |ctx, queue| async {
    a2a_protocol_server::EventEmitter::new(ctx, queue)
        .status(a2a_protocol_types::TaskState::Working)
        .await?;
    std::future::pending().await
});

/// The client `builder` builds, recording into a fresh reader.
fn instrumented(builder: ClientBuilder) -> (A2aClient, Arc<ManualReader>, SdkMeterProvider) {
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    let meter = opentelemetry::metrics::MeterProvider::meter(&provider, "a2a.client");
    let mut client = builder.build().expect("client");
    client.metric = CallMetric::from_meter(&meter);
    (client, reader, provider)
}

/// A JSON-RPC client of `addr`, recording into a fresh reader.
fn client_of(addr: std::net::SocketAddr) -> (A2aClient, Arc<ManualReader>, SdkMeterProvider, u16) {
    let (client, reader, provider) = instrumented(ClientBuilder::new(format!("http://{addr}")));
    (client, reader, provider, addr.port())
}

/// A JSON-RPC client of a fresh server, recording into a fresh reader.
async fn client() -> (A2aClient, Arc<ManualReader>, SdkMeterProvider, u16) {
    client_running(Done).await
}

/// [`client`], of a server running `executor`.
async fn client_running(
    executor: impl a2a_protocol_server::AgentExecutor,
) -> (A2aClient, Arc<ManualReader>, SdkMeterProvider, u16) {
    let handler = RequestHandlerBuilder::new(executor)
        .without_agent_card()
        .build()
        .expect("handler");
    let addr = a2a_protocol_server::serve_with_addr(
        "127.0.0.1:0",
        JsonRpcDispatcher::new(Arc::new(handler)),
    )
    .await
    .expect("serve");
    client_of(addr)
}

/// An address that accepts a connection and never answers it, so a call to
/// it ends only when the caller gives up.
async fn silent_peer() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let _held = socket;
                std::future::pending::<()>().await;
            });
        }
    });
    addr
}

fn send() -> MessageSendParams {
    MessageSendParams::new(Message::user("m-1", vec![Part::text("hi")]))
}

fn attrs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut v: Vec<_> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    v.sort();
    v
}

/// Each call records one point with the conventions' attributes: a success
/// none of `error.type` or (on JSON-RPC) `rpc.status_code`; a failure the
/// JSON-RPC code it was answered with as both.
#[tokio::test]
async fn each_call_records_its_duration_with_the_conventions_attributes() {
    let (client, reader, _provider, port) = client().await;
    let port = port.to_string();
    client.send_message(send()).await.expect("send");
    let missing = TaskQueryParams {
        tenant: None,
        id: "no-such-task".into(),
        history_length: None,
    };
    client.get_task(missing).await.expect_err("unknown task");

    let mut seen: Vec<_> = points(&reader)
        .into_iter()
        .map(|(a, n, _)| (a, n))
        .collect();
    seen.sort();
    let common = [
        ("rpc.system.name", "jsonrpc"),
        ("server.address", "127.0.0.1"),
        ("server.port", port.as_str()),
    ];
    let mut failed = common.to_vec();
    failed.extend([
        ("rpc.method", "lf.a2a.v1.A2AService/GetTask"),
        ("rpc.status_code", "-32001"),
        ("error.type", "-32001"),
    ]);
    let mut sent = common.to_vec();
    sent.push(("rpc.method", "lf.a2a.v1.A2AService/SendMessage"));
    let mut want = vec![(attrs(&failed), 1), (attrs(&sent), 1)];
    want.sort();
    assert_eq!(seen, want);
}

/// A streaming call lasts until its stream is dropped, as its span does: no
/// point before, and one after whose duration covers the time it was held.
#[tokio::test]
async fn a_streaming_call_is_measured_until_its_stream_is_dropped() {
    const HELD: Duration = Duration::from_millis(60);
    let (client, reader, _provider, _) = client().await;
    let mut stream = client.stream_message(send()).await.expect("stream");
    while let Some(event) = stream.next().await {
        event.expect("event");
    }
    tokio::time::sleep(HELD).await;
    assert!(
        points(&reader).is_empty(),
        "recorded before the stream was dropped"
    );
    drop(stream);
    let points = points(&reader);
    assert_eq!(points.len(), 1, "{points:?}");
    let (attributes, count, sum) = &points[0];
    assert_eq!(*count, 1);
    assert!(
        attributes.contains(&(
            "rpc.method".to_owned(),
            "lf.a2a.v1.A2AService/SendStreamingMessage".to_owned()
        )),
        "{attributes:?}"
    );
    assert!(
        !attributes.iter().any(|(k, _)| k == "error.type"),
        "{attributes:?}"
    );
    assert!(
        *sum >= HELD.as_secs_f64(),
        "measured {sum} s, held {HELD:?}"
    );
}

/// A call the caller gives up on — its future dropped by a timeout or a
/// `select!` — is recorded, as `cancelled` and with the time it ran, as the
/// server records a call whose peer went away. Unary and streaming alike: a
/// stream is dropped before it opens, here.
#[tokio::test]
async fn a_call_the_caller_drops_is_recorded_as_cancelled() {
    const WAITED: Duration = Duration::from_millis(60);
    // The call's clock starts a few microseconds after `timeout` computes its
    // deadline, and tokio's timer ticks in whole milliseconds, so a call
    // dropped by the timeout ran for a little less than the wait (59.8 ms of
    // 60 on a macOS runner). The slack keeps that out of the assertion and
    // still fails for a duration that is zero or measured from the wrong clock.
    const SLACK: Duration = Duration::from_millis(10);
    let addr = silent_peer().await;
    let (client, reader, _provider, port) = client_of(addr);
    let port = port.to_string();

    let sent = tokio::time::timeout(WAITED, client.send_message(send())).await;
    assert!(sent.is_err(), "the peer never answers");
    let streamed = tokio::time::timeout(WAITED, client.stream_message(send())).await;
    assert!(streamed.is_err(), "the peer never answers");

    let seen = points(&reader);
    let want = |method: &str| {
        attrs(&[
            ("rpc.system.name", "jsonrpc"),
            ("rpc.method", method),
            ("error.type", "cancelled"),
            ("server.address", "127.0.0.1"),
            ("server.port", port.as_str()),
        ])
    };
    let mut wanted = vec![
        want("lf.a2a.v1.A2AService/SendMessage"),
        want("lf.a2a.v1.A2AService/SendStreamingMessage"),
    ];
    wanted.sort();
    let mut names: Vec<_> = seen.iter().map(|p| p.0.clone()).collect();
    names.sort();
    assert_eq!(names, wanted, "{seen:?}");
    for (attributes, count, sum) in &seen {
        assert_eq!(*count, 1, "{attributes:?}");
        assert!(
            *sum >= WAITED.saturating_sub(SLACK).as_secs_f64(),
            "{attributes:?} measured {sum} s, waited {WAITED:?}"
        );
    }
}

/// A stream the caller drops before it has ended — before a final event or
/// the end of the body — is a call the caller cut short, and is recorded as
/// `cancelled` when it is dropped, as the server records the same stream
/// when its peer goes away. One read to the end is not (see
/// `a_streaming_call_is_measured_until_its_stream_is_dropped`).
#[tokio::test]
async fn a_stream_dropped_before_it_ended_is_recorded_as_cancelled() {
    let (client, reader, _provider, _) = client_running(Endless).await;
    let mut stream = client.stream_message(send()).await.expect("stream");
    stream.next().await.expect("an event").expect("ok");
    assert!(points(&reader).is_empty(), "recorded before the drop");
    drop(stream);

    let points = points(&reader);
    assert_eq!(points.len(), 1, "{points:?}");
    let (attributes, count, _) = &points[0];
    assert_eq!(*count, 1);
    assert!(
        attributes.contains(&("error.type".to_owned(), "cancelled".to_owned())),
        "{attributes:?}"
    );
    assert!(
        attributes.contains(&(
            "rpc.method".to_owned(),
            "lf.a2a.v1.A2AService/SendStreamingMessage".to_owned()
        )),
        "{attributes:?}"
    );
}

/// An interceptor that refuses every answer.
struct RefuseAfter;

#[allow(clippy::unused_async_trait_impl)]
impl crate::interceptor::CallInterceptor for RefuseAfter {
    async fn before<'a>(
        &'a self,
        _: &'a mut crate::interceptor::ClientRequest,
    ) -> crate::ClientResult<()> {
        Ok(())
    }
    async fn after<'a>(
        &'a self,
        _: &'a crate::interceptor::ClientResponse,
    ) -> crate::ClientResult<()> {
        Err(crate::error::ClientError::InvalidAgentResponse(
            "refused".into(),
        ))
    }
}

/// The one point `reader` holds, as its `error.type`.
fn only_error_type(reader: &ManualReader) -> Option<String> {
    let points = points(reader);
    assert_eq!(points.len(), 1, "{points:?}");
    let (attributes, count, _) = &points[0];
    assert_eq!(*count, 1, "recorded more than once: {attributes:?}");
    attributes
        .iter()
        .find(|(k, _)| k == "error.type")
        .map(|(_, v)| v.clone())
}

/// A call is recorded as it ended for the caller, not as the agent answered
/// it: an interceptor that refuses the answer after the agent gave it makes
/// a failed call, recorded once.
#[tokio::test]
async fn a_response_an_interceptor_refuses_is_recorded_as_failed() {
    let handler = RequestHandlerBuilder::new(Done)
        .without_agent_card()
        .build()
        .expect("handler");
    let addr = a2a_protocol_server::serve_with_addr(
        "127.0.0.1:0",
        JsonRpcDispatcher::new(Arc::new(handler)),
    )
    .await
    .expect("serve");
    let (client, reader, _provider) =
        instrumented(ClientBuilder::new(format!("http://{addr}")).with_interceptor(RefuseAfter));
    client.send_message(send()).await.expect_err("refused");
    assert_eq!(
        only_error_type(&reader).as_deref(),
        Some("invalid_response")
    );
}

/// Answers every call with a result no method decodes.
struct Garbage;

impl crate::transport::Transport for Garbage {
    fn send_request<'a>(
        &'a self,
        _: &'a str,
        _: serde_json::Value,
        _: &'a std::collections::HashMap<String, String>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::ClientResult<serde_json::Value>> + Send + 'a>,
    > {
        Box::pin(async { Ok(serde_json::json!("not a task")) })
    }
    fn send_streaming_request<'a>(
        &'a self,
        _: &'a str,
        _: serde_json::Value,
        _: &'a std::collections::HashMap<String, String>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = crate::ClientResult<crate::streaming::EventStream>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(crate::error::ClientError::Transport("unused".into())) })
    }
    fn rpc_system(&self) -> Option<&'static str> {
        Some("jsonrpc")
    }
}

/// An answer the client cannot decode is a failed call, whatever the agent
/// thinks it sent.
#[tokio::test]
async fn a_response_that_does_not_decode_is_recorded_as_failed() {
    let (client, reader, _provider) =
        instrumented(ClientBuilder::new("http://127.0.0.1:9").with_custom_transport(Garbage));
    let missing = TaskQueryParams {
        tenant: None,
        id: "t".into(),
        history_length: None,
    };
    client.get_task(missing).await.expect_err("does not decode");
    assert_eq!(only_error_type(&reader).as_deref(), Some("serialization"));
}

/// A stream whose `after` hook refuses it fails its call with the refusal;
/// it was not abandoned by its caller, who never saw it.
#[tokio::test]
async fn a_stream_an_interceptor_refuses_is_recorded_as_failed() {
    let handler = RequestHandlerBuilder::new(Done)
        .without_agent_card()
        .build()
        .expect("handler");
    let addr = a2a_protocol_server::serve_with_addr(
        "127.0.0.1:0",
        JsonRpcDispatcher::new(Arc::new(handler)),
    )
    .await
    .expect("serve");
    let (client, reader, _provider) =
        instrumented(ClientBuilder::new(format!("http://{addr}")).with_interceptor(RefuseAfter));
    client.stream_message(send()).await.expect_err("refused");
    assert_eq!(
        only_error_type(&reader).as_deref(),
        Some("invalid_response")
    );
}

/// `Debug` names the type and nothing it holds: a provider is not printable
/// and a histogram says nothing a reader of a client's `{:?}` can use.
#[test]
fn debug_names_the_type_only() {
    assert_eq!(format!("{:?}", CallMetric::global()), "CallMetric { .. }");
}
