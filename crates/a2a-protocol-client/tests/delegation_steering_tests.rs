// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `Delegation::steer` and `steer_all` (ADR 0036), against this repository's
//! server over JSON-RPC: a parent steers a child that works, and the child's
//! executor reads what it said.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_client::delegation::{Delegation, Outcome, Steering, steer_all};
use a2a_protocol_client::{A2aClient, ClientBuilder, ClientError};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{
    AgentExecutor, Dispatcher, EventEmitter, RequestContext, SteeringConfig,
};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::idempotency;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::steering::{self, Delivery};
use a2a_protocol_types::{Message, MessageSendParams, TaskState};

const GUARD: Duration = Duration::from_secs(20);

/// What a worker's executor read from its inbox.
#[derive(Debug, Clone)]
struct Seen {
    text: String,
    task_id: String,
    context_id: Option<String>,
    declared: bool,
    key: Option<String>,
}

/// What the message that starts a task says to do:
/// `read`: read two messages, then complete;
/// `read-one`: read one, then complete;
/// `block`: never read; works until cancelled;
/// `park`: park at `input-required`;
/// `finish`: complete at once.
struct Worker {
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl AgentExecutor for Worker {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            match ctx.message.text().unwrap_or_default() {
                "finish" => return emit.status(TaskState::Completed).await,
                "park" => return emit.status(TaskState::InputRequired).await,
                _ => {}
            }
            emit.status(TaskState::Working).await?;
            let want = match ctx.message.text().unwrap_or_default() {
                "read" => 2,
                "read-one" => 1,
                _ => {
                    ctx.cancellation_token.cancelled().await;
                    return Ok(());
                }
            };
            let Some(inbox) = ctx.inbox() else {
                ctx.cancellation_token.cancelled().await;
                return Ok(());
            };
            for _ in 0..want {
                let Some(steered) = inbox.recv().await else {
                    break;
                };
                let message = steered.message;
                self.seen.lock().unwrap().push(Seen {
                    text: message.text().unwrap_or_default().to_owned(),
                    task_id: message
                        .task_id
                        .as_ref()
                        .map(|t| t.0.clone())
                        .unwrap_or_default(),
                    context_id: message.context_id.as_ref().map(|c| c.0.clone()),
                    declared: steering::declares_extension(&message),
                    key: idempotency::key_of(&message)
                        .ok()
                        .flatten()
                        .map(str::to_owned),
                });
            }
            emit.status(TaskState::Completed).await
        })
    }
}

async fn serve(config: Option<SteeringConfig>) -> (SocketAddr, Arc<Mutex<Vec<Seen>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mut builder = RequestHandlerBuilder::new(Worker {
        seen: Arc::clone(&seen),
    })
    .without_agent_card();
    if let Some(config) = config {
        builder = builder.with_steering(config);
    }
    let handler = Arc::new(builder.build().expect("handler"));
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
    (addr, seen)
}

fn client(addr: SocketAddr) -> Arc<A2aClient> {
    Arc::new(
        ClientBuilder::new(format!("http://{addr}"))
            .with_peer_honouring_idempotency(true)
            .build()
            .expect("client"),
    )
}

fn say(text: &str) -> Message {
    Message::user_text(uuid::Uuid::new_v4().to_string(), text)
}

/// Starts a child that does `what` and reads events until the agent has named
/// it.
async fn child(client: &Arc<A2aClient>, what: &str) -> Delegation {
    let mut child = Delegation::start(Arc::clone(client), MessageSendParams::new(say(what)))
        .await
        .expect("start");
    tokio::time::timeout(GUARD, async {
        while child.task_id().is_none() {
            child.next_event().await.expect("an event").expect("ok");
        }
    })
    .await
    .expect("the child is named");
    child
}

async fn seen_n(seen: &Arc<Mutex<Vec<Seen>>>, n: usize) -> Vec<Seen> {
    let deadline = std::time::Instant::now() + GUARD;
    loop {
        {
            let seen = seen.lock().unwrap();
            if seen.len() >= n {
                return seen.clone();
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the worker read fewer than {n}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_parent_steers_its_child_and_the_childs_executor_reads_it() {
    let (addr, seen) = serve(Some(SteeringConfig::new())).await;
    let client = client(addr);
    let child = child(&client, "read-one").await;
    let task = child.task_id().expect("named").to_owned();

    let steered = child
        .steer(say("use the cheaper index"))
        .await
        .expect("steered");
    match steered {
        Steering::Delivered {
            delivery,
            task: answer,
        } => {
            assert_eq!(delivery, Delivery::Inbox);
            assert_eq!(answer.id.0, task);
        }
        other => panic!("expected a delivery, got {other:?}"),
    }
    let read = seen_n(&seen, 1).await;
    assert_eq!(read[0].text, "use the cheaper index");
    assert_eq!(read[0].task_id, task, "the message names the child's task");
    assert!(read[0].context_id.is_some(), "and its context");
    assert!(read[0].declared, "and declares the extension");
    let key = read[0]
        .key
        .as_deref()
        .expect("an idempotency key was added");
    assert_eq!(key.len(), 64, "a fresh key: {key}");

    let done = tokio::time::timeout(GUARD, child.wait(std::future::pending()))
        .await
        .expect("settles");
    assert!(
        matches!(done.outcome, Outcome::Completed(_)),
        "{:?}",
        done.outcome
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_not_yet_named_is_not_steered_and_nothing_is_sent() {
    let (addr, seen) = serve(Some(SteeringConfig::new())).await;
    let client = client(addr);
    let child = Delegation::start(client, MessageSendParams::new(say("read-one")))
        .await
        .expect("start");
    assert!(child.task_id().is_none());
    assert!(child.steer_handle().is_none());
    let refused = child.steer(say("too early")).await;
    match refused {
        Err(ClientError::Transport(text)) => {
            assert!(text.contains("has not been named"), "{text}");
            assert!(text.contains("nothing was sent"), "{text}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(seen.lock().unwrap().is_empty());
    let _ = child.cancel().await;
}

/// An agent that does not take steering refuses the message as it refuses
/// any message to a working task; the parent gets that refusal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_that_does_not_take_steering_refuses_as_it_always_did() {
    let (addr, seen) = serve(None).await;
    let client = client(addr);
    let child = child(&client, "block").await;
    let refused = child.steer(say("hello")).await;
    match refused {
        Err(ClientError::Protocol(error)) => {
            assert!(
                error.message.contains("already being processed"),
                "{error:?}"
            );
        }
        other => panic!("expected the agent's refusal, got {other:?}"),
    }
    assert!(seen.lock().unwrap().is_empty());
    let _ = child.cancel().await;
}

/// A child parked at `input-required` is continued by the message, as any
/// message to it would; the answer says it was not steering.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_that_parked_is_continued_not_steered() {
    let (addr, _seen) = serve(Some(SteeringConfig::new())).await;
    let client = client(addr);
    let mut child = child(&client, "park").await;
    tokio::time::timeout(GUARD, async {
        while !child.is_settled() {
            if child.next_event().await.is_none() {
                break;
            }
        }
    })
    .await
    .expect("the child parks");
    let handle = child.steer_handle().expect("named");
    assert_eq!(
        Some(handle.task_id()),
        child.task_id(),
        "the handle names the child's task"
    );
    // The continuation starts a turn that parks again.
    let mut message = say("park");
    message.role = a2a_protocol_types::MessageRole::User;
    match handle.steer(message).await.expect("answered") {
        Steering::NotSteered(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::InputRequired);
        }
        other => panic!("expected the continuation's answer, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retried_steer_with_its_key_is_delivered_once() {
    let (addr, seen) = serve(Some(SteeringConfig::new())).await;
    let client = client(addr);
    let child = child(&client, "read").await;
    let handle = child.steer_handle().expect("named");

    let mut message = say("only once");
    idempotency::set_key(&mut message, "0123456789abcdef0123456789abcdef").unwrap();
    let first = handle.steer(message.clone()).await.expect("delivered");
    assert!(
        matches!(
            first,
            Steering::Delivered {
                delivery: Delivery::Inbox,
                ..
            }
        ),
        "{first:?}"
    );
    let retry = handle.steer(message).await.expect("answered");
    assert!(
        matches!(
            retry,
            Steering::Delivered {
                delivery: Delivery::Replayed,
                ..
            }
        ),
        "{retry:?}"
    );
    // A second, different message is the one the executor was waiting for.
    handle.steer(say("and this")).await.expect("delivered");
    let read = seen_n(&seen, 2).await;
    assert_eq!(
        read.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        ["only once", "and this"],
        "the retry was not delivered again"
    );
    let done = tokio::time::timeout(GUARD, child.wait(std::future::pending()))
        .await
        .expect("settles");
    assert!(matches!(done.outcome, Outcome::Completed(_)));
}

/// A full inbox is refused, with the reason, and not waited for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_inbox_is_refused_to_the_parent() {
    let (addr, _seen) = serve(Some(SteeringConfig::new().with_inbox_capacity(1))).await;
    let client = client(addr);
    let child = child(&client, "block").await;
    child.steer(say("one")).await.expect("room");
    let refused = tokio::time::timeout(GUARD, child.steer(say("two")))
        .await
        .expect("a full inbox does not hold the parent");
    let error = refused.expect_err("refused");
    assert!(error.to_string().contains("steering inbox"), "{error}");
    let _ = child.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn steer_all_reports_each_child_in_order() {
    let (addr, seen) = serve(Some(SteeringConfig::new())).await;
    let client = client(addr);
    let one = child(&client, "read-one").await;
    let two = child(&client, "read-one").await;
    // A child that has finished: steering it is refused.
    let mut done = child(&client, "finish").await;
    tokio::time::timeout(GUARD, async {
        while !done.is_settled() {
            if done.next_event().await.is_none() {
                break;
            }
        }
    })
    .await
    .expect("settles");
    // A child nothing has named yet.
    let unnamed = Delegation::start(Arc::clone(&client), MessageSendParams::new(say("block")))
        .await
        .expect("start");
    let children = [one, done, two, unnamed];
    let ids: Vec<Option<String>> = children
        .iter()
        .map(|c| c.task_id().map(str::to_owned))
        .collect();

    let made = std::sync::atomic::AtomicUsize::new(0);
    let outcomes = steer_all(&children, || {
        let n = made.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        say(&format!("correction {n}"))
    })
    .await;

    assert_eq!(outcomes.len(), 4);
    assert_eq!(
        outcomes
            .iter()
            .map(|o| o.task_id.clone())
            .collect::<Vec<_>>(),
        ids,
        "one outcome per child, in the order given"
    );
    assert!(
        matches!(
            outcomes[0].result,
            Ok(Steering::Delivered {
                delivery: Delivery::Inbox,
                ..
            })
        ),
        "{:?}",
        outcomes[0].result
    );
    assert!(
        matches!(&outcomes[1].result, Err(ClientError::Protocol(e)) if e.message.contains("terminal state")),
        "{:?}",
        outcomes[1].result
    );
    assert!(
        matches!(
            outcomes[2].result,
            Ok(Steering::Delivered {
                delivery: Delivery::Inbox,
                ..
            })
        ),
        "{:?}",
        outcomes[2].result
    );
    assert!(
        matches!(&outcomes[3].result, Err(ClientError::Transport(t)) if t.contains("has not been named")),
        "{:?}",
        outcomes[3].result
    );
    assert_eq!(
        made.load(std::sync::atomic::Ordering::SeqCst),
        3,
        "a message is made for each child that can be sent one"
    );
    let read = seen_n(&seen, 2).await;
    let mut texts: Vec<&str> = read.iter().map(|s| s.text.as_str()).collect();
    texts.sort_unstable();
    assert_eq!(texts.len(), 2);
    assert!(
        texts.iter().all(|t| t.starts_with("correction ")),
        "{texts:?}"
    );
    assert_ne!(texts[0], texts[1], "each child got a message of its own");
    for child in children {
        let _ = child.cancel().await;
    }
}

#[test]
fn the_steering_uri_is_the_one_the_adr_reserves() {
    assert_eq!(
        steering::STEERING_EXTENSION_URI,
        "https://a2a-rust.com/extensions/steering/v1"
    );
}
