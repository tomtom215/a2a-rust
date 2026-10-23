// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `Delegation` and idempotency keys (ADR 0026), against this repository's
//! server: a key is added when the agent's card says it honours one, the
//! same message sent again with it reaches the same child, and nothing is
//! added for an agent not known to honour keys.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_client::delegation::{Delegation, Outcome};
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::store::TenantAwareInMemoryTaskStore;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{AgentExecutor, Dispatcher, EventEmitter, RequestContext};
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::idempotency::{self, IDEMPOTENCY_EXTENSION_URI};
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskState};

const GUARD: Duration = Duration::from_secs(20);

/// Counts its runs, and settles without ending, so the child stays for a
/// second send to find.
struct Worker(Arc<AtomicUsize>);

impl AgentExecutor for Worker {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            EventEmitter::new(ctx, queue)
                .status(TaskState::InputRequired)
                .await
        })
    }
}

/// The worker, serving JSON-RPC at the address returned, the card it
/// serves, and its run count.
async fn serve() -> (SocketAddr, AgentCard, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let card = AgentCard::new(
        "worker",
        "1.0.0",
        AgentInterface::jsonrpc(format!("http://{addr}")),
    )
    .with_description("asks for input")
    .with_streaming(true)
    .with_skill(AgentSkill::new("s", "S", "d").with_tags(["t"]));
    let handler = Arc::new(
        RequestHandlerBuilder::new(Worker(Arc::clone(&runs)))
            .with_agent_card(card)
            .with_task_store(TenantAwareInMemoryTaskStore::new())
            .build()
            .expect("handler"),
    );
    let dispatcher: Arc<dyn Dispatcher> = Arc::new(JsonRpcDispatcher::new(handler));
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
    // The card as a client sees it, extensions the builder added included.
    let served = a2a_protocol_client::discovery::resolve_agent_card(&format!("http://{addr}"))
        .await
        .expect("the served card");
    (addr, served, runs)
}

fn params(id: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user(id, vec![Part::text("work")]))
}

/// Starts a delegation and waits for it to settle, returning its key and
/// the child's task id.
async fn delegate(client: &Arc<A2aClient>, params: MessageSendParams) -> (Option<String>, String) {
    let child = Delegation::start(Arc::clone(client), params)
        .await
        .expect("start");
    let key = child.idempotency_key().map(str::to_owned);
    let done = tokio::time::timeout(GUARD, child.wait(std::future::pending()))
        .await
        .expect("settles");
    assert!(
        matches!(done.outcome, Outcome::Interrupted(_)),
        "{:?}",
        done.outcome
    );
    (key, done.task_id.expect("a task id"))
}

#[tokio::test]
async fn a_key_is_added_for_an_agent_whose_card_honours_one_and_reaches_the_same_child() {
    let (_, card, runs) = serve().await;
    assert!(
        card.capabilities
            .extensions
            .iter()
            .flatten()
            .any(|e| e.uri == IDEMPOTENCY_EXTENSION_URI),
        "the server advertises the extension"
    );
    let client = Arc::new(
        ClientBuilder::from_card(&card)
            .expect("card")
            .build()
            .expect("client"),
    );
    assert!(client.honours_idempotency());

    let (key, first) = delegate(&client, params("m-1")).await;
    let key = key.expect("a key was added");
    assert_eq!(key.len(), 64);
    assert!(idempotency::validate_key(&key).is_ok());

    assert_eq!(runs.load(Ordering::SeqCst), 1);
    let mut again = params("m-1");
    idempotency::set_key(&mut again.message, &key).expect("valid");
    let (kept, second) = delegate(&client, again).await;
    assert_eq!(
        kept.as_deref(),
        Some(key.as_str()),
        "the caller's key is kept"
    );
    assert_eq!(
        second, first,
        "the same message with its key is the same child"
    );
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "and the worker did not run again"
    );

    // Another delegation of the same text is new work, with a new key.
    let (other_key, third) = delegate(&client, params("m-2")).await;
    assert_ne!(other_key.as_deref(), Some(key.as_str()));
    assert_ne!(third, first);
}

#[tokio::test]
async fn nothing_is_added_for_an_agent_not_known_to_honour_keys() {
    let (addr, _, _) = serve().await;
    let client = Arc::new(
        ClientBuilder::new(format!("http://{addr}"))
            .build()
            .expect("client"),
    );
    assert!(!client.honours_idempotency());
    let (key, _) = delegate(&client, params("m-3")).await;
    assert_eq!(key, None);
}
