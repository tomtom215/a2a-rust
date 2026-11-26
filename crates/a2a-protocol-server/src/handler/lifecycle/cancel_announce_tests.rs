// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A `CancelTask` of a task no turn runs tells its log (and so a subscriber on
//! another replica, woken at the append) that it ended; one whose turn may run
//! elsewhere leaves that to the turn (COR-02, register E-F25).

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::params::CancelTaskParams;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;
use crate::handler::RequestHandler;
use crate::store::claim::{HeldClaim, InMemoryTurnClaims, TurnClaimStore};
use crate::store::tenant::TenantContext;
use crate::store::{InMemoryTaskStore, TaskStore};

struct Cancelable;
agent_executor!(Cancelable,
    execute: |_ctx, _queue| async { Ok(()) },
    cancel: |_ctx, _queue| async { Ok(()) }
);

type Fut<'a, T> = std::pin::Pin<
    Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<T>> + Send + 'a>,
>;

/// Claims shared between the handler and the test, which plays the replica
/// that holds one.
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
}

/// A handler over a store holding task `t` in `state`, with turn claims when
/// `claims` is given.
async fn handler(state: TaskState, claims: Option<&Arc<InMemoryTurnClaims>>) -> RequestHandler {
    let store = Arc::new(InMemoryTaskStore::new());
    store
        .save(&Task::new(
            TaskId::new("t"),
            ContextId::new("c"),
            TaskStatus::new(state),
        ))
        .await
        .expect("seed");
    let builder = RequestHandlerBuilder::new(Cancelable)
        .without_agent_card()
        .with_task_store_arc(store);
    match claims {
        Some(claims) => builder.with_turn_claims(Shared(Arc::clone(claims))),
        None => builder,
    }
    .build()
    .expect("builds")
}

/// The states the task's log holds, in order.
async fn logged(h: &RequestHandler) -> Vec<TaskState> {
    h.task_store
        .read_events(&TaskId::new("t"), 0, 100)
        .await
        .expect("log")
        .into_iter()
        .filter_map(|e| match e.event {
            StreamResponse::StatusUpdate(u) => Some(u.status.state),
            _ => None,
        })
        .collect()
}

async fn cancel(h: &RequestHandler) {
    let task = h
        .on_cancel_task(CancelTaskParams::new("t".to_owned()), None)
        .await
        .expect("cancelable");
    assert_eq!(task.status.state, TaskState::Canceled);
}

/// The case E-F25 named: a task parked between turns, with no claim store.
/// Before COR-02 the log stayed empty and a subscriber elsewhere learned of
/// the cancel only at its next poll.
#[tokio::test]
async fn a_parked_task_cancelled_is_logged_as_ended() {
    let h = handler(TaskState::InputRequired, None).await;
    cancel(&h).await;
    assert_eq!(logged(&h).await, vec![TaskState::Canceled]);
}

/// With no claim store, a task that is `working` may be running on another
/// replica, whose processor tells its log: this cancel does not, so the
/// ending is not told twice.
#[tokio::test]
async fn a_working_task_without_claims_is_left_to_its_turn() {
    let h = handler(TaskState::Working, None).await;
    cancel(&h).await;
    assert_eq!(logged(&h).await, vec![]);
}

/// With claims, and no live claim on the task, no turn runs it anywhere
/// (its replica is gone): the cancel tells the log.
#[tokio::test]
async fn a_task_no_claim_holds_is_logged_as_ended() {
    let claims = Arc::new(InMemoryTurnClaims::new());
    let h = handler(TaskState::Working, Some(&claims)).await;
    cancel(&h).await;
    assert_eq!(logged(&h).await, vec![TaskState::Canceled]);
}

/// With a live claim held by another replica, that replica's turn is asked
/// to stop, and its processor tells the log: this cancel does not.
#[tokio::test]
async fn a_task_another_replica_holds_is_left_to_its_turn() {
    let claims = Arc::new(InMemoryTurnClaims::new());
    assert!(
        claims
            .claim(
                &TenantContext::partition(),
                "t",
                "replica-b",
                "token-b",
                Duration::from_secs(60)
            )
            .await
            .expect("claim")
    );
    let h = handler(TaskState::Working, Some(&claims)).await;
    cancel(&h).await;
    assert_eq!(logged(&h).await, vec![]);
}

/// Records the state of every status it is asked to deliver.
struct Recorder(Arc<std::sync::Mutex<Vec<TaskState>>>);

impl crate::push::PushSender for Recorder {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        event: &'a StreamResponse,
        _config: &'a a2a_protocol_types::push::TaskPushNotificationConfig,
    ) -> Fut<'a, ()> {
        if let StreamResponse::StatusUpdate(e) = event {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(e.status.state);
        }
        Box::pin(async { Ok(()) })
    }
}

/// The webhooks of a parked task hear the cancel. Before COR-02 nothing on
/// this path pushed: the cancel's writer is a throwaway with no push, and no
/// processor runs a parked task.
#[tokio::test]
async fn a_parked_task_cancelled_is_told_to_its_webhooks() {
    let store = Arc::new(InMemoryTaskStore::new());
    store
        .save(&Task::new(
            TaskId::new("t"),
            ContextId::new("c"),
            TaskStatus::new(TaskState::InputRequired),
        ))
        .await
        .expect("seed");
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let h = RequestHandlerBuilder::new(Cancelable)
        .without_agent_card()
        .with_task_store_arc(store)
        .with_push_sender(Recorder(Arc::clone(&sent)))
        .build()
        .expect("builds");
    h.push_config_store
        .set(
            a2a_protocol_types::push::TaskPushNotificationConfig::new(
                "t",
                "https://example.com/hook",
            )
            .with_id("cfg"),
        )
        .await
        .expect("config");

    cancel(&h).await;

    assert_eq!(
        *sent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![TaskState::Canceled]
    );
}

/// An ending durably told raises no warning: the warning is for one that was
/// not, which an operator reads as webhooks and a log that never heard.
#[cfg(feature = "tracing")]
#[test]
fn an_ending_told_raises_no_warning() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime");
    let warnings = crate::handler::shutdown::tests::warning::warnings_during(|| {
        rt.block_on(async {
            let h = handler(TaskState::InputRequired, None).await;
            cancel(&h).await;
            assert_eq!(logged(&h).await, vec![TaskState::Canceled]);
        });
    });
    assert!(
        !warnings.iter().any(|w| w.contains("was not durably told")),
        "a told ending was warned about: {warnings:?}"
    );
}
