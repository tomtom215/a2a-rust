// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use super::*;

const TTL: Duration = Duration::from_secs(60);

#[tokio::test]
async fn a_live_claim_is_exclusive_and_released_only_by_its_token() {
    let claims = InMemoryTurnClaims::new();
    assert!(claims.claim("", "t", "a", "tok-a", TTL).await.unwrap());
    assert!(
        !claims.claim("", "t", "b", "tok-b", TTL).await.unwrap(),
        "a live claim is not taken"
    );
    assert!(
        !claims.claim("", "t", "a", "tok-a2", TTL).await.unwrap(),
        "not even by its own owner's next turn"
    );
    assert!(
        claims.claim("other", "t", "b", "tok-c", TTL).await.unwrap(),
        "another partition's task of the same id is a different claim"
    );
    claims.release("tok-b").await.unwrap();
    assert!(claims.holds("tok-a"), "a stale token releases nothing");
    claims.release("tok-a").await.unwrap();
    assert!(claims.claim("", "t", "b", "tok-b", TTL).await.unwrap());
}

#[tokio::test]
async fn an_expired_claim_is_taken_over_and_its_owner_learns_at_renewal() {
    let claims = InMemoryTurnClaims::new();
    assert!(
        claims
            .claim("", "t", "a", "tok-a", Duration::from_millis(20))
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert!(claims.claim("", "t", "b", "tok-b", TTL).await.unwrap());
    assert!(
        claims
            .renew("a", &["tok-a".to_owned()], TTL)
            .await
            .unwrap()
            .is_empty(),
        "the old owner's renewal no longer reports the claim"
    );
    assert_eq!(
        claims.renew("b", &["tok-b".to_owned()], TTL).await.unwrap(),
        [HeldClaim {
            token: "tok-b".to_owned(),
            cancel_requested: false
        }]
    );
}

#[tokio::test]
async fn a_cancel_request_marks_only_a_live_claim() {
    let claims = InMemoryTurnClaims::new();
    assert!(
        !claims.request_cancel("", "t").await.unwrap(),
        "nothing to mark"
    );
    assert!(claims.claim("", "t", "a", "tok", TTL).await.unwrap());
    assert!(claims.request_cancel("", "t").await.unwrap());
    let report = claims.renew("a", &["tok".to_owned()], TTL).await.unwrap();
    assert!(report[0].cancel_requested);
}

#[tokio::test]
async fn a_sweep_removes_only_long_expired_claims() {
    let claims = InMemoryTurnClaims::new();
    assert!(
        claims
            .claim("", "old", "a", "tok-old", Duration::from_millis(1))
            .await
            .unwrap()
    );
    assert!(claims.claim("", "new", "a", "tok-new", TTL).await.unwrap());
    tokio::time::sleep(Duration::from_millis(30)).await;
    // Expired, but more recently than a minute ago: kept.
    assert_eq!(claims.sweep(Duration::from_secs(60)).await.unwrap(), 0);
    assert_eq!(claims.sweep(Duration::from_millis(10)).await.unwrap(), 1);
    assert!(claims.holds("tok-new"));
    assert!(!claims.holds("tok-old"));
}

#[tokio::test]
async fn a_renewal_extends_only_the_tokens_it_names() {
    let claims = InMemoryTurnClaims::new();
    let short = Duration::from_millis(20);
    assert!(
        claims
            .claim("", "kept", "a", "tok-kept", short)
            .await
            .unwrap()
    );
    assert!(
        claims
            .claim("", "orphan", "a", "tok-orphan", short)
            .await
            .unwrap()
    );
    let report = claims
        .renew("a", &["tok-kept".to_owned()], TTL)
        .await
        .unwrap();
    assert_eq!(report.len(), 1, "{report:?}");
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert!(
        claims.claim("", "orphan", "b", "tok-b", TTL).await.unwrap(),
        "a claim its owner no longer names expires"
    );
    assert!(
        !claims.claim("", "kept", "b", "tok-b2", TTL).await.unwrap(),
        "a named one was extended"
    );
}

#[test]
fn a_claim_lost_error_round_trips_and_is_not_a_terminal_conflict() {
    let err = ClaimLost::new(TaskId::new("t")).into_error();
    assert_eq!(
        ClaimLost::from_error(&err),
        Some(ClaimLost::new(TaskId::new("t")))
    );
    assert!(crate::store::TerminalStateConflict::from_error(&err).is_none());
    assert!(ClaimLost::from_error(&A2aError::internal("other")).is_none());
}

#[cfg(any(feature = "sqlite", feature = "postgres"))]
#[test]
fn the_fence_condition_names_its_placeholder_twice() {
    assert_eq!(
        fence_sql!("?4", "?1"),
        "(?4 IS NULL OR EXISTS (SELECT 1 FROM a2a_task_claims c WHERE c.token = ?4 \
         AND c.task_id = ?1))"
    );
    #[cfg(feature = "postgres")]
    assert_eq!(
        fence_sql!(postgres "$4::text", "$1"),
        "(set_config('a2a.claims', 'on', true) = 'on' AND ($4::text IS NULL OR \
         EXISTS (SELECT 1 FROM a2a_task_claims c WHERE c.token = $4::text \
         AND c.task_id = $1 FOR KEY SHARE)))"
    );
}

/// Under the claim token of task A, a write to task A lands and a write to
/// task B is refused as a lost claim: the fence is the claim on the task
/// written, not any claim. Before 2026-10-29 a token holding a claim on any
/// task let the write through.
#[cfg(any(feature = "sqlite", feature = "postgres"))]
async fn a_token_fences_only_its_own_task(store: &dyn crate::store::TaskStore) {
    use a2a_protocol_types::task::{ContextId, Task, TaskState, TaskStatus};

    use crate::store::tenant::TenantContext;

    let run = uuid::Uuid::new_v4().to_string();
    let (a, b, token) = (
        format!("fenced-a-{run}"),
        format!("fenced-b-{run}"),
        format!("tok-{run}"),
    );
    let task = |id: &str| Task {
        id: TaskId::new(id),
        context_id: ContextId::new("fenced"),
        status: TaskStatus::new(TaskState::Working),
        history: None,
        artifacts: None,
        metadata: None,
    };
    // B exists before the turn, so its append is refused by the fence and
    // not by a missing row.
    store.save(&task(&b)).await.expect("B, outside any turn");
    let claims = store.turn_claims().expect("a SQL store has claims");
    assert!(claims.claim("", &a, "me", &token, TTL).await.unwrap());

    TenantContext::scope_request("", async {
        assert!(TenantContext::set_fence(&token));
        store
            .save(&task(&a))
            .await
            .expect("the claimed task is written");
        let err = store
            .save(&task(&b))
            .await
            .expect_err("another task is not written under A's claim");
        assert!(ClaimLost::from_error(&err).is_some(), "{err}");
        appends_are_fenced_to_their_own_task(store, &a, &b).await;
    })
    .await;
    claims.release(&token).await.unwrap();
}

/// The appends of `a_token_fences_only_its_own_task`, under the claim on `a`:
/// one that inserts nothing is a replay while the claim holds, and refused
/// once it does not.
#[cfg(any(feature = "sqlite", feature = "postgres"))]
async fn appends_are_fenced_to_their_own_task(
    store: &dyn crate::store::TaskStore,
    a: &str,
    b: &str,
) {
    use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
    use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

    let event = |id: &str| {
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new(id),
            context_id: ContextId::new("fenced"),
            status: TaskStatus::new(TaskState::Working),
            metadata: None,
        })
    };
    for _ in 0..2 {
        store
            .append_event(&TaskId::new(a), 1, &event(a))
            .await
            .expect("the claimed task's log is appended to, and the replay is no error");
    }
    let err = store
        .append_event(&TaskId::new(b), 1, &event(b))
        .await
        .expect_err("another task's log is not appended to under A's claim");
    assert!(ClaimLost::from_error(&err).is_some(), "{err}");
    assert_eq!(
        store
            .read_events(&TaskId::new(b), 0, 10)
            .await
            .unwrap()
            .len(),
        0,
        "another task's log is not appended to under A's claim"
    );
}

/// A cancel for a task nobody holds marks nothing and says so: the caller
/// reads `false` as "no turn to stop here", so a `true` would send it waiting
/// for a cancel nothing will act on.
#[cfg(any(feature = "sqlite", feature = "postgres"))]
async fn a_cancel_marks_only_a_held_claim(store: &dyn crate::store::TaskStore) {
    let run = uuid::Uuid::new_v4().to_string();
    let (task, token) = (format!("cancel-{run}"), format!("tok-{run}"));
    let claims = store.turn_claims().expect("a SQL store has claims");
    assert!(
        !claims.request_cancel("", &task).await.unwrap(),
        "nothing to mark"
    );
    assert!(claims.claim("", &task, "me", &token, TTL).await.unwrap());
    assert!(claims.request_cancel("", &task).await.unwrap());
    claims.release(&token).await.unwrap();
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_fences_a_token_to_its_own_task() {
    let store = crate::store::SqliteTaskStore::new("sqlite::memory:")
        .await
        .unwrap();
    a_token_fences_only_its_own_task(&store).await;
    a_cancel_marks_only_a_held_claim(&store).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn postgres_fences_a_token_to_its_own_task() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let store = crate::store::PostgresTaskStore::new(&url).await.unwrap();
    a_token_fences_only_its_own_task(&store).await;
    a_cancel_marks_only_a_held_claim(&store).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn tenant_aware_postgres_fences_a_token_to_its_own_task() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let store = crate::store::TenantAwarePostgresTaskStore::new(&url)
        .await
        .unwrap();
    a_token_fences_only_its_own_task(&store).await;
    a_cancel_marks_only_a_held_claim(&store).await;
}

/// Only `'claimed'` grants the turn, and only `'legacy'` says an earlier
/// release's turn holds it, which this replica logs while it waits.
#[cfg(feature = "postgres")]
#[test]
fn the_claim_answer_is_read_one_meaning_each() {
    use super::postgres::read_claim_outcome;
    assert_eq!(read_claim_outcome("claimed"), (true, false));
    assert_eq!(read_claim_outcome("legacy"), (false, true));
    assert_eq!(read_claim_outcome("held"), (false, false));
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn tenant_aware_sqlite_fences_a_token_to_its_own_task() {
    let store = crate::store::TenantAwareSqliteTaskStore::new("sqlite::memory:")
        .await
        .unwrap();
    a_token_fences_only_its_own_task(&store).await;
    a_cancel_marks_only_a_held_claim(&store).await;
}

/// A replica waiting on an earlier release's turn says so, once a minute
/// rather than at every poll. The subscriber is this thread's only, so other
/// tests running beside this one cannot add to or take from what it counts.
#[cfg(all(feature = "postgres", feature = "tracing"))]
#[test]
fn an_earlier_releases_turn_is_logged_once_a_minute() {
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

    struct Lines(Arc<Mutex<Vec<String>>>);
    impl<S: tracing::Subscriber> Layer<S> for Lines {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            struct Message(String);
            impl tracing::field::Visit for Message {
                fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                    if f.name() == "message" {
                        self.0 = format!("{v:?}");
                    }
                }
            }
            let mut m = Message(String::new());
            event.record(&mut m);
            self.0.lock().unwrap().push(m.0);
        }
    }

    let lines = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(Lines(Arc::clone(&lines)));
    tracing::subscriber::with_default(subscriber, || {
        super::postgres::legacy_turn_seen("t-1");
        super::postgres::legacy_turn_seen("t-2");
    });
    let said = lines
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains("a replica of an earlier release is running this task"))
        .count();
    assert_eq!(said, 1, "{:?}", lines.lock().unwrap());
}

/// A claim holds until the instant it expires and not at it, where a real
/// clock can almost never land; constructed instants pin the boundary.
#[test]
fn a_claim_is_free_at_the_instant_it_expires() {
    let t = std::time::Instant::now();
    assert!(super::live(t + Duration::from_millis(1), t));
    assert!(!super::live(t, t), "free to take at the instant it expires");
    assert!(!super::live(t, t + Duration::from_millis(1)));
}
