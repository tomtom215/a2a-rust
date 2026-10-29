// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use super::*;
use crate::store::{HeldClaim, InMemoryTurnClaims};

fn claims() -> TurnClaims {
    TurnClaims::new(
        Arc::new(InMemoryTurnClaims::new()),
        Some("test".to_owned()),
        DEFAULT_TURN_CLAIM_TTL,
        DEFAULT_TURN_CLAIM_RENEWAL,
    )
}

fn hold(claims: &TurnClaims, token: &str) -> CancellationToken {
    let cancel = CancellationToken::new();
    claims.held().insert(
        token.to_owned(),
        Held {
            task_id: TaskId::new(format!("task-{token}")),
            cancel: Some(cancel.clone()),
        },
    );
    cancel
}

#[test]
fn a_cancel_asked_for_elsewhere_fires_the_executors_token() {
    let claims = claims();
    let asked = hold(&claims, "a");
    let quiet = hold(&claims, "b");
    act_on(
        &claims,
        &[
            HeldClaim {
                token: "a".to_owned(),
                cancel_requested: true,
            },
            HeldClaim {
                token: "b".to_owned(),
                cancel_requested: false,
            },
        ],
    );
    assert!(
        asked.is_cancelled(),
        "the asked-for cancel reaches the token"
    );
    assert!(
        !quiet.is_cancelled(),
        "a claim with no request is left running"
    );
    assert_eq!(claims.held().len(), 2, "both claims are still held");
}

#[test]
fn a_claim_missing_from_the_renewal_was_lost_and_stops_its_executor() {
    let claims = claims();
    let lost = hold(&claims, "a");
    let kept = hold(&claims, "b");
    act_on(
        &claims,
        &[HeldClaim {
            token: "b".to_owned(),
            cancel_requested: false,
        }],
    );
    assert!(lost.is_cancelled(), "a lost claim cancels its executor");
    assert!(!kept.is_cancelled());
    assert!(
        !claims.held().contains_key("a"),
        "a lost claim is no longer renewed"
    );
}

#[test]
fn the_owner_names_the_replica_and_is_unique_per_handler() {
    let a = claims();
    let b = claims();
    assert!(a.owner.starts_with("test/"), "{}", a.owner);
    assert_ne!(a.owner, b.owner, "two handlers never share an owner");
}

#[tokio::test]
async fn a_dropped_claim_is_released() {
    let store = Arc::new(InMemoryTurnClaims::new());
    let claims = Arc::new(TurnClaims::new(
        Arc::clone(&store) as Arc<dyn TurnClaimStore>,
        None,
        DEFAULT_TURN_CLAIM_TTL,
        DEFAULT_TURN_CLAIM_RENEWAL,
    ));
    assert!(
        store
            .claim("", "t", &claims.owner, "tok", DEFAULT_TURN_CLAIM_TTL)
            .await
            .unwrap()
    );
    hold(&claims, "tok");
    drop(TurnClaim {
        claims: Arc::clone(&claims),
        token: "tok".to_owned(),
    });
    for _ in 0..100 {
        if !store.holds("tok") {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(!store.holds("tok"), "Drop releases the claim");
    assert!(claims.held().is_empty());
}
