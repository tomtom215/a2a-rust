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
        &["a".to_owned(), "b".to_owned()],
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
        &["a".to_owned(), "b".to_owned()],
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
fn a_claim_taken_while_a_renewal_was_in_flight_is_not_lost() {
    // The renewal was sent holding only "a"; "b" was claimed before its
    // answer came back, so the answer cannot name it. Judging "b" by that
    // answer would cancel a turn that holds its claim.
    let claims = claims();
    let a = hold(&claims, "a");
    let sent = vec!["a".to_owned()];
    let b = hold(&claims, "b");
    act_on(
        &claims,
        &sent,
        &[HeldClaim {
            token: "a".to_owned(),
            cancel_requested: false,
        }],
    );
    assert!(!a.is_cancelled());
    assert!(
        !b.is_cancelled(),
        "a claim newer than the renewal is left running"
    );
    assert!(
        claims.held().contains_key("b"),
        "and is renewed at the next interval"
    );
}

#[test]
fn a_claim_released_while_a_renewal_was_in_flight_is_skipped() {
    let claims = claims();
    let a = hold(&claims, "a");
    let sent = vec!["a".to_owned()];
    claims.held().remove("a");
    act_on(&claims, &sent, &[]);
    assert!(!a.is_cancelled(), "a released turn is not cancelled");
}

#[test]
fn the_owner_names_the_replica_and_is_unique_per_handler() {
    let a = claims();
    let b = claims();
    assert!(a.owner.starts_with("test/"), "{}", a.owner);
    assert_ne!(a.owner, b.owner, "two handlers never share an owner");
}

#[test]
fn a_claims_debug_names_its_owner_and_not_its_token() {
    let claims = Arc::new(claims());
    let claim = TurnClaim {
        claims: Arc::clone(&claims),
        token: "tok-secret".to_owned(),
    };
    let shown = format!("{claim:?}");
    assert_eq!(
        shown,
        format!("TurnClaim {{ owner: {:?}, .. }}", claims.owner)
    );
    assert!(!shown.contains("tok-secret"), "{shown}");
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

/// A handler given no replica name owns its claims as the host it runs on,
/// which Kubernetes sets to the pod's name, else `replica`, then the process
/// and a fresh id. Read here rather
/// than set, since the environment is the process's.
#[test]
fn an_unnamed_replica_owns_its_claims_as_its_host() {
    let claims = TurnClaims::new(
        Arc::new(InMemoryTurnClaims::new()),
        None,
        DEFAULT_TURN_CLAIM_TTL,
        DEFAULT_TURN_CLAIM_RENEWAL,
    );
    let host = std::env::var("HOSTNAME").unwrap_or_default();
    let expected = if host.is_empty() { "replica" } else { &host };
    let prefix = format!("{expected}/{}/", std::process::id());
    assert!(
        claims.owner.starts_with(&prefix),
        "{} for {prefix}",
        claims.owner
    );
}

/// An empty `HOSTNAME` names nothing, so it is `replica` as an unset one is.
/// Tested apart from the environment, which a test runner may not export.
#[test]
fn an_empty_host_name_is_no_name() {
    assert_eq!(replica_name(Some("pod-1".to_owned())), "pod-1");
    assert_eq!(replica_name(Some(String::new())), "replica");
    assert_eq!(replica_name(None), "replica");
}

/// Counts claim attempts on a store that never grants one, and refuses to
/// count past a cap: a wait that stopped growing would poll without end.
struct NeverGranted(Arc<std::sync::atomic::AtomicUsize>);

impl crate::store::TurnClaimStore for NeverGranted {
    fn claim<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: &'a str,
        _: &'a str,
        _: Duration,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<bool>>
                + Send
                + 'a,
        >,
    > {
        let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        assert!(
            n <= 50,
            "{n} claim attempts in one wait: the backoff stopped growing"
        );
        Box::pin(async { Ok(false) })
    }
    fn release<'a>(
        &'a self,
        _: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
    fn renew<'a>(
        &'a self,
        _: &'a str,
        _: &'a [String],
        _: Duration,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<Vec<HeldClaim>>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn request_cancel<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<bool>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(false) })
    }
    fn sweep(
        &self,
        _: Duration,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<u64>> + Send + '_,
        >,
    > {
        Box::pin(async { Ok(0) })
    }
}

/// A send waiting on another replica's turn polls with a growing wait, 20 ms
/// doubling to 250 ms, until the drain timeout: about eight attempts in a
/// second, not a busy loop against the database.
#[tokio::test(start_paused = true)]
async fn a_send_waiting_on_another_replica_backs_off() {
    struct Idle;
    crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handler = crate::RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_turn_claims(NeverGranted(Arc::clone(&attempts)))
        .with_handler_limits(
            crate::handler::HandlerLimits::default()
                .with_executor_drain_timeout(Duration::from_secs(1)),
        )
        .build()
        .expect("handler");
    let refused = handler.claim_turn(&TaskId::new("held-elsewhere")).await;
    assert!(
        refused.is_err(),
        "the turn is never granted, so the wait ends refused"
    );
    let attempts = attempts.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        (5..=12).contains(&attempts),
        "{attempts} attempts in one second"
    );
}
