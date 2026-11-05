// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `Delegation` and lineage (ADR 0034), against this repository's server
//! over JSON-RPC: the child receives the lineage the parent gave it, an
//! operator's halt of the job's root stops the child and the delegation sees
//! it canceled, and a send of the halted job is refused with the
//! machine-readable `HALTED` detail (ADR 0049).

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_client::delegation::{Delegation, DelegationOptions, Outcome};
use a2a_protocol_client::{ClientBuilder, ClientError};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::store::TenantAwareInMemoryTaskStore;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{
    AgentExecutor, Dispatcher, EventEmitter, HaltScope, RequestContext, RequestHandler,
};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::lineage::{LINEAGE_EXTENSION_URI, Lineage};
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskState};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

const GUARD: Duration = Duration::from_secs(20);

/// Works until cancelled, reporting the lineage it was started with.
struct Worker(UnboundedSender<Lineage>);

impl AgentExecutor for Worker {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            EventEmitter::new(ctx, queue)
                .status(TaskState::Working)
                .await?;
            let _ = self.0.send(ctx.lineage().clone());
            ctx.cancellation_token.cancelled().await;
            Ok(())
        })
    }
}

/// The worker's handler, the address it serves JSON-RPC on, and the
/// lineages its turns start with.
async fn serve() -> (Arc<RequestHandler>, SocketAddr, UnboundedReceiver<Lineage>) {
    let (tx, rx) = unbounded_channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handler = Arc::new(
        RequestHandlerBuilder::new(Worker(tx))
            .without_agent_card()
            .with_task_store(TenantAwareInMemoryTaskStore::new())
            .build()
            .expect("handler"),
    );
    let dispatcher: Arc<dyn Dispatcher> = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));
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
    (handler, addr, rx)
}

fn params() -> MessageSendParams {
    MessageSendParams::new(Message::user(
        uuid::Uuid::new_v4().to_string(),
        vec![Part::text("work")],
    ))
}

#[tokio::test]
async fn a_halt_of_the_root_stops_the_delegated_child_and_refuses_the_job() {
    let (worker, addr, mut started) = serve().await;
    let client = Arc::new(
        ClientBuilder::new(format!("http://{addr}"))
            .build()
            .expect("client"),
    );
    // What a parent's executor gets from `RequestContext::child_lineage`.
    let lineage = Lineage::origin("job-7").child("parent-1").expect("a child");
    let child = Delegation::start_with(
        Arc::clone(&client),
        params(),
        DelegationOptions::new().with_lineage(lineage.clone()),
    )
    .await
    .expect("start");
    let seen = tokio::time::timeout(GUARD, started.recv())
        .await
        .expect("the child started")
        .expect("the worker is alive");
    assert_eq!(
        seen, lineage,
        "the child runs with the lineage it was given"
    );

    let report = worker
        .halt(HaltScope::root("", "job-7"), "oncall", "job 7 runs away")
        .await;
    assert_eq!(report.stopped.len(), 1);
    let done = tokio::time::timeout(GUARD, child.wait(std::future::pending()))
        .await
        .expect("settles");
    assert!(
        matches!(done.outcome, Outcome::Canceled(_)),
        "{:?}",
        done.outcome
    );

    let mut again = params();
    lineage.attach(&mut again.message);
    let error = client.send_message(again).await.expect_err("halted");
    let ClientError::Protocol(error) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(error.message, "halted: job 7 runs away");
    let details = error.data.expect("details");
    let details = details.as_array().expect("a details array");
    assert!(
        details.iter().any(|d| d["reason"] == "HALTED"
            && d["domain"] == "a2a-rust.com"
            && d["@type"] == "type.googleapis.com/google.rpc.ErrorInfo"),
        "{details:?}"
    );

    // Another job is not halted.
    let mut other = params();
    Lineage::origin("job-8").attach(&mut other.message);
    let mut other_params = other;
    other_params.configuration = Some(
        serde_json::from_value(serde_json::json!({ "returnImmediately": true })).expect("config"),
    );
    client.send_message(other_params).await.expect("job 8 runs");
    assert_eq!(
        tokio::time::timeout(GUARD, started.recv())
            .await
            .expect("started"),
        Some(Lineage::origin("job-8"))
    );
}

#[test]
fn the_lineage_option_is_printed() {
    let options = DelegationOptions::new().with_lineage(Lineage::origin("job"));
    assert!(
        format!("{options:?}").ends_with(
            "lineage: Some(Lineage { root: \"job\", depth: 0, parent_task_id: None }) }"
        ),
        "{options:?}"
    );
    assert_eq!(
        LINEAGE_EXTENSION_URI,
        "https://a2a-rust.com/extensions/lineage/v1"
    );
}
