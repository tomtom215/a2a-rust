// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Steering across replicas (ADR 0036): two handlers over one store and one
//! set of turn claims, as two replicas have, with the claims in memory. The
//! same cases run against SQLite and PostgreSQL in `tests/turn_claims`.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::steering::{Delivery, Receipt};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use super::steer_tests::{
    Agent, GUARD, Mode, Probe, Recorder, background, message, reaches, send, steering_to, wait_for,
};
use crate::RequestHandler;
use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::steering::SteeringConfig;
use crate::store::{Handed, HeldClaim, PendingSteering};
use crate::store::{InMemoryTaskStore, InMemoryTurnClaims, TaskStore, TurnClaimStore};

type Fut<'a, T> = std::pin::Pin<
    Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<T>> + Send + 'a>,
>;

/// The shared claims, handed to each replica as its own store of them.
struct Shared(Arc<InMemoryTurnClaims>);

impl TurnClaimStore for Shared {
    fn claim<'a>(
        &'a self,
        scope: &'a str,
        task_id: &'a str,
        owner: &'a str,
        token: &'a str,
        ttl: Duration,
    ) -> Fut<'a, bool> {
        self.0.claim(scope, task_id, owner, token, ttl)
    }
    fn release<'a>(&'a self, token: &'a str) -> Fut<'a, ()> {
        self.0.release(token)
    }
    fn renew<'a>(
        &'a self,
        owner: &'a str,
        tokens: &'a [String],
        ttl: Duration,
    ) -> Fut<'a, Vec<HeldClaim>> {
        self.0.renew(owner, tokens, ttl)
    }
    fn request_cancel<'a>(&'a self, scope: &'a str, task_id: &'a str) -> Fut<'a, bool> {
        self.0.request_cancel(scope, task_id)
    }
    fn sweep(&self, older_than: Duration) -> Fut<'_, u64> {
        self.0.sweep(older_than)
    }
    fn hand_steering<'a>(
        &'a self,
        scope: &'a str,
        task_id: &'a str,
        message: &'a str,
        capacity: usize,
    ) -> Fut<'a, Handed> {
        self.0.hand_steering(scope, task_id, message, capacity)
    }
    fn peek_steering<'a>(
        &'a self,
        owner: &'a str,
        tokens: &'a [String],
        limit: usize,
    ) -> Fut<'a, Vec<PendingSteering>> {
        self.0.peek_steering(owner, tokens, limit)
    }
    fn ack_steering<'a>(&'a self, seqs: &'a [i64]) -> Fut<'a, ()> {
        self.0.ack_steering(seqs)
    }
}

/// A claim store that keeps claims and says nothing about steering: what a
/// store written before the extension does.
struct Before(Arc<InMemoryTurnClaims>);

impl TurnClaimStore for Before {
    fn claim<'a>(
        &'a self,
        scope: &'a str,
        task_id: &'a str,
        owner: &'a str,
        token: &'a str,
        ttl: Duration,
    ) -> Fut<'a, bool> {
        self.0.claim(scope, task_id, owner, token, ttl)
    }
    fn release<'a>(&'a self, token: &'a str) -> Fut<'a, ()> {
        self.0.release(token)
    }
    fn renew<'a>(
        &'a self,
        owner: &'a str,
        tokens: &'a [String],
        ttl: Duration,
    ) -> Fut<'a, Vec<HeldClaim>> {
        self.0.renew(owner, tokens, ttl)
    }
    fn request_cancel<'a>(&'a self, scope: &'a str, task_id: &'a str) -> Fut<'a, bool> {
        self.0.request_cancel(scope, task_id)
    }
    fn sweep(&self, older_than: Duration) -> Fut<'_, u64> {
        self.0.sweep(older_than)
    }
}

struct Replica {
    handler: RequestHandler,
    probe: Arc<Probe>,
    metrics: Arc<Recorder>,
}

fn replica(
    store: &Arc<dyn TaskStore>,
    claims: impl TurnClaimStore,
    mode: Mode,
    config: SteeringConfig,
) -> Replica {
    let probe = Arc::new(Probe::default());
    let metrics = Arc::new(Recorder::default());
    let handler = RequestHandlerBuilder::new(Agent {
        mode,
        probe: Arc::clone(&probe),
    })
    .without_agent_card()
    .with_task_store_arc(Arc::clone(store))
    .with_turn_claims(claims)
    // A short renewal, so what is handed over is looked at within 50 ms.
    .with_turn_claim_timing(Duration::from_secs(3), Duration::from_millis(50))
    .with_steering(config)
    .with_metrics(Arc::clone(&metrics))
    .build()
    .expect("handler");
    Replica {
        handler,
        probe,
        metrics,
    }
}

fn shared_store() -> Arc<dyn TaskStore> {
    Arc::new(InMemoryTaskStore::new())
}

/// Starts a task on `on` and waits until its executor runs.
async fn start(on: &Replica) -> TaskId {
    let task = send(&on.handler, background(message(None, "work")))
        .await
        .expect("starts");
    tokio::time::timeout(GUARD, on.probe.started.notified())
        .await
        .expect("running");
    task.id
}

/// The headline of the cross-replica design: a message sent to the replica
/// that does not hold the claim reaches the executor on the one that does.
#[tokio::test]
async fn a_message_to_the_other_replica_reaches_the_executor_on_the_one_that_holds_the_claim() {
    let store = shared_store();
    let claims = Arc::new(InMemoryTurnClaims::new());
    let a = replica(
        &store,
        Shared(Arc::clone(&claims)),
        Mode::Read(2),
        SteeringConfig::new(),
    );
    let b = replica(
        &store,
        Shared(Arc::clone(&claims)),
        Mode::Block,
        SteeringConfig::new(),
    );
    let id = start(&a).await;

    let first = send(&b.handler, steering_to(&id.0, "from B, first"))
        .await
        .expect("B accepts it");
    assert_eq!(
        Receipt::read(&first).unwrap(),
        Some(Receipt::new(Delivery::HandedOff)),
        "B says it handed the message over, not that it delivered it"
    );
    send(&b.handler, steering_to(&id.0, "from B, second"))
        .await
        .expect("B accepts it");

    reaches(&store, &id, TaskState::Completed).await;
    assert_eq!(
        *a.probe.read.lock().unwrap(),
        ["from B, first", "from B, second"],
        "A's executor read both, in order"
    );
    assert_eq!(b.metrics.outcomes(), ["handed_off", "handed_off"]);
    assert_eq!(a.metrics.count("received"), 2);
    assert!(b.probe.read.lock().unwrap().is_empty(), "B ran nothing");
}

/// With the claim gone (the replica died, or its claim lapsed) the message is
/// refused as transient: nothing is delivered and the sender is told so, so
/// the message is not lost.
#[tokio::test]
async fn with_the_claim_lapsed_the_message_is_refused_not_lost() {
    let store = shared_store();
    let claims = Arc::new(InMemoryTurnClaims::new());
    let b = replica(
        &store,
        Shared(Arc::clone(&claims)),
        Mode::Block,
        SteeringConfig::new(),
    );
    // A task left `working` by a replica that is gone: no turn, no claim.
    store
        .save(&Task::new(
            TaskId::new("orphan"),
            ContextId::new("c"),
            TaskStatus::new(TaskState::Working),
        ))
        .await
        .unwrap();

    let mut params = steering_to("orphan", "is anyone there");
    a2a_protocol_types::idempotency::set_key(&mut params.message, "0123456789abcdef0123").unwrap();
    let refused = send(&b.handler, params.clone()).await;
    match &refused {
        Err(error @ ServerError::Overloaded(text)) => {
            assert!(
                text.contains("ended before the steering message was delivered"),
                "{text}"
            );
            assert_eq!(error.http_status(), 503);
            assert!(error.retry_after().is_some());
        }
        other => panic!("expected a transient refusal, got {other:?}"),
    }
    assert_eq!(b.metrics.outcomes(), ["ended"]);
    // Nothing is waiting in the store for a claim that may come later, and
    // the key is free for the retry.
    assert_eq!(claims.queued(), 0);
    let again = send(&b.handler, params).await;
    assert!(
        matches!(again, Err(ServerError::Overloaded(_))),
        "{again:?}"
    );
}

/// A replica whose claim store cannot hand a message over says so, in words
/// that name the cause, and delivers nothing.
#[tokio::test]
async fn a_claim_store_that_cannot_hand_over_refuses_in_words_that_say_why() {
    let store = shared_store();
    let claims = Arc::new(InMemoryTurnClaims::new());
    let a = replica(
        &store,
        Shared(Arc::clone(&claims)),
        Mode::Block,
        SteeringConfig::new(),
    );
    let b = replica(
        &store,
        Before(Arc::clone(&claims)),
        Mode::Block,
        SteeringConfig::new(),
    );
    let id = start(&a).await;
    match send(&b.handler, steering_to(&id.0, "x")).await {
        Err(ServerError::UnsupportedOperation(text)) => {
            assert_eq!(
                text,
                format!(
                    "task {id} is being processed by another replica, and the turn claim \
                     store this server uses cannot hand it a steering message"
                )
            );
        }
        other => panic!("expected UnsupportedOperation, got {other:?}"),
    }
    assert_eq!(b.metrics.outcomes(), Vec::<String>::new());
    a.probe.release.notify_one();
}

/// A replica that halted refuses to hand steering to a job that is halted
/// for it, as it refuses any send of that job.
#[tokio::test]
async fn a_halted_replica_hands_nothing_over() {
    let store = shared_store();
    let claims = Arc::new(InMemoryTurnClaims::new());
    let a = replica(
        &store,
        Shared(Arc::clone(&claims)),
        Mode::Block,
        SteeringConfig::new(),
    );
    let b = replica(
        &store,
        Shared(Arc::clone(&claims)),
        Mode::Block,
        SteeringConfig::new(),
    );
    let id = start(&a).await;
    let _ = b
        .handler
        .halt(crate::HaltScope::All, "operator", "test")
        .await;
    let refused = send(&b.handler, steering_to(&id.0, "x")).await;
    assert!(
        matches!(refused, Err(ServerError::Halted(_))),
        "{refused:?}"
    );
    assert_eq!(b.metrics.outcomes(), ["halted"]);
    assert_eq!(claims.queued(), 0, "nothing was stored for the halted job");
    a.probe.release.notify_one();
}

/// The store's queue is bounded by the same capacity as the inbox: a task
/// holds at most twice its capacity, nothing waiting in the store is lost
/// when the inbox is full, and the message past both is refused with 503.
#[tokio::test]
async fn a_full_inbox_delays_a_handed_over_message_and_a_full_store_refuses_it() {
    let store = shared_store();
    let claims = Arc::new(InMemoryTurnClaims::new());
    let config = SteeringConfig::new().with_inbox_capacity(2);
    let a = replica(
        &store,
        Shared(Arc::clone(&claims)),
        Mode::ReadAfterRelease(4),
        config,
    );
    let b = replica(&store, Shared(Arc::clone(&claims)), Mode::Block, config);
    let id = start(&a).await;

    for text in ["m1", "m2"] {
        send(&b.handler, steering_to(&id.0, text))
            .await
            .expect("room in the store");
    }
    // A moves them into its inbox, which is then full: the store has room again.
    wait_for("A's inbox holds the first two", || {
        a.metrics.count("received") == 2
    })
    .await;
    for text in ["m3", "m4"] {
        send(&b.handler, steering_to(&id.0, text))
            .await
            .expect("room in the store");
    }
    let refused = send(&b.handler, steering_to(&id.0, "m5")).await;
    assert!(
        matches!(refused, Err(ServerError::Overloaded(_))),
        "inbox full and store full: {refused:?}"
    );
    assert_eq!(
        b.metrics.outcomes(),
        [
            "handed_off",
            "handed_off",
            "handed_off",
            "handed_off",
            "full"
        ]
    );
    assert_eq!(
        a.metrics.count("received"),
        2,
        "the inbox is full, so m3 and m4 wait in the store"
    );

    // Released, the executor reads four, in the order they were sent.
    a.probe.release.notify_one();
    reaches(&store, &id, TaskState::Completed).await;
    assert_eq!(*a.probe.read.lock().unwrap(), ["m1", "m2", "m3", "m4"]);
    assert_eq!(a.metrics.count("received"), 4);
}

/// The replica that holds the claim also takes messages itself, and they go
/// straight to the inbox, not through the store.
#[tokio::test]
async fn the_replica_that_holds_the_claim_delivers_to_its_own_inbox_directly() {
    let store = shared_store();
    let claims = Arc::new(InMemoryTurnClaims::new());
    let a = replica(
        &store,
        Shared(Arc::clone(&claims)),
        Mode::Read(1),
        SteeringConfig::new(),
    );
    let id = start(&a).await;
    let answer = send(&a.handler, steering_to(&id.0, "direct"))
        .await
        .expect("delivered");
    assert_eq!(
        Receipt::read(&answer).unwrap(),
        Some(Receipt::new(Delivery::Inbox))
    );
    reaches(&store, &id, TaskState::Completed).await;
    assert_eq!(*a.probe.read.lock().unwrap(), ["direct"]);
    assert_eq!(a.metrics.outcomes(), ["delivered"]);
}
