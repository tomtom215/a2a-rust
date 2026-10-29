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
        claims.renew("a", TTL).await.unwrap().is_empty(),
        "the old owner's renewal no longer reports the claim"
    );
    assert_eq!(
        claims.renew("b", TTL).await.unwrap(),
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
    let report = claims.renew("a", TTL).await.unwrap();
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
    assert_eq!(claims.sweep(Duration::from_millis(10)).await.unwrap(), 1);
    assert!(claims.holds("tok-new"));
    assert!(!claims.holds("tok-old"));
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
        fence_sql!("?4"),
        "(?4 IS NULL OR EXISTS (SELECT 1 FROM a2a_task_claims c WHERE c.token = ?4))"
    );
}
