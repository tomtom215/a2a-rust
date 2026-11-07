// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Handing steering messages to the replica that holds a claim (ADR 0036),
//! in each claim store: one set of cases, run against the in-memory store,
//! SQLite (plain and tenant-aware) and PostgreSQL (`#[ignore]`d without
//! `A2A_TEST_POSTGRES_URL`).

use std::sync::Arc;
use std::time::Duration;

use super::{Handed, TurnClaimStore};

pub(super) const LONG: Duration = Duration::from_secs(60);

/// A task id and claim names no other case shares, so cases may share a
/// database.
pub(super) struct Ids {
    pub(super) task: String,
    pub(super) owner: String,
}

pub(super) fn fresh() -> Ids {
    let tag = uuid::Uuid::new_v4().simple().to_string();
    Ids {
        task: format!("task-{tag}"),
        owner: format!("owner-{tag}"),
    }
}

fn token() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub(super) async fn claimed(claims: &dyn TurnClaimStore, scope: &str, ids: &Ids) -> String {
    let token = token();
    assert!(
        claims
            .claim(scope, &ids.task, &ids.owner, &token, LONG)
            .await
            .unwrap(),
        "the claim is free"
    );
    token
}

async fn texts(
    claims: &dyn TurnClaimStore,
    ids: &Ids,
    tokens: &[String],
    limit: usize,
) -> Vec<String> {
    claims
        .peek_steering(&ids.owner, tokens, limit)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.message)
        .collect()
}

async fn nothing_is_stored_for_a_task_nobody_holds(claims: &dyn TurnClaimStore) {
    let ids = fresh();
    assert_eq!(
        claims.hand_steering("", &ids.task, "a", 10).await.unwrap(),
        Handed::NoClaim
    );
    // A claim taken afterwards finds nothing: the refusal stored nothing.
    let t = claimed(claims, "", &ids).await;
    assert_eq!(texts(claims, &ids, &[t], 10).await, Vec::<String>::new());
}

async fn a_message_waits_for_the_claims_owner_in_order(claims: &dyn TurnClaimStore) {
    let ids = fresh();
    let t = claimed(claims, "", &ids).await;
    for m in ["a", "b", "c"] {
        assert_eq!(
            claims.hand_steering("", &ids.task, m, 10).await.unwrap(),
            Handed::Queued
        );
    }
    let all = claims
        .peek_steering(&ids.owner, std::slice::from_ref(&t), 10)
        .await
        .unwrap();
    let messages: Vec<&str> = all.iter().map(|p| p.message.as_str()).collect();
    assert_eq!(messages, ["a", "b", "c"], "oldest first");
    assert!(all.iter().all(|p| p.token == t));
    assert!(
        all.windows(2).all(|w| w[0].seq < w[1].seq),
        "the sequence orders them: {all:?}"
    );

    // Reading is not taking: the same look gives the same answer.
    assert_eq!(
        texts(claims, &ids, std::slice::from_ref(&t), 10)
            .await
            .len(),
        3
    );
    // A limit cuts from the oldest.
    assert_eq!(
        texts(claims, &ids, std::slice::from_ref(&t), 2).await,
        ["a", "b"]
    );
    assert_eq!(
        texts(claims, &ids, std::slice::from_ref(&t), 1).await,
        ["a"]
    );
    // Only its owner, and only for the tokens it names.
    let other = Ids {
        task: ids.task.clone(),
        owner: format!("{}-other", ids.owner),
    };
    assert_eq!(
        texts(claims, &other, std::slice::from_ref(&t), 10).await,
        Vec::<String>::new()
    );
    assert_eq!(
        texts(claims, &ids, &[token()], 10).await,
        Vec::<String>::new()
    );
    assert_eq!(texts(claims, &ids, &[], 10).await, Vec::<String>::new());

    // Acknowledged messages are gone, the rest stay; a name that is not
    // there is ignored.
    claims.ack_steering(&[all[0].seq, i64::MAX]).await.unwrap();
    assert_eq!(
        texts(claims, &ids, std::slice::from_ref(&t), 10).await,
        ["b", "c"]
    );
    claims.ack_steering(&[]).await.unwrap();
    assert_eq!(
        texts(claims, &ids, std::slice::from_ref(&t), 10).await,
        ["b", "c"]
    );
}

/// The queue holds `capacity` and not one more, at 0, 1 and 2, and a message
/// read and acknowledged makes room for exactly one.
async fn the_queue_is_bounded_by_the_capacity(claims: &dyn TurnClaimStore) {
    let ids = fresh();
    let t = claimed(claims, "", &ids).await;
    assert_eq!(
        claims.hand_steering("", &ids.task, "x", 0).await.unwrap(),
        Handed::Full,
        "a capacity of 0 holds nothing"
    );
    assert_eq!(
        claims.hand_steering("", &ids.task, "a", 2).await.unwrap(),
        Handed::Queued
    );
    assert_eq!(
        claims.hand_steering("", &ids.task, "b", 2).await.unwrap(),
        Handed::Queued
    );
    assert_eq!(
        claims.hand_steering("", &ids.task, "c", 2).await.unwrap(),
        Handed::Full
    );
    assert_eq!(
        texts(claims, &ids, std::slice::from_ref(&t), 10).await,
        ["a", "b"]
    );
    let first = claims
        .peek_steering(&ids.owner, std::slice::from_ref(&t), 1)
        .await
        .unwrap();
    claims.ack_steering(&[first[0].seq]).await.unwrap();
    assert_eq!(
        claims.hand_steering("", &ids.task, "d", 2).await.unwrap(),
        Handed::Queued
    );
    assert_eq!(
        claims.hand_steering("", &ids.task, "e", 2).await.unwrap(),
        Handed::Full
    );
    assert_eq!(
        texts(claims, &ids, std::slice::from_ref(&t), 10).await,
        ["b", "d"]
    );
}

/// A claim is scoped to a tenant's partition, and so is a message to it.
async fn a_message_is_for_the_scope_of_the_claim(claims: &dyn TurnClaimStore) {
    let ids = fresh();
    let t = claimed(claims, "acme", &ids).await;
    assert_eq!(
        claims.hand_steering("", &ids.task, "x", 10).await.unwrap(),
        Handed::NoClaim,
        "no tenant is not acme"
    );
    assert_eq!(
        claims
            .hand_steering("globex", &ids.task, "x", 10)
            .await
            .unwrap(),
        Handed::NoClaim,
        "another tenant's task is not found"
    );
    assert_eq!(
        texts(claims, &ids, std::slice::from_ref(&t), 10).await,
        Vec::<String>::new()
    );
    assert_eq!(
        claims
            .hand_steering("acme", &ids.task, "x", 10)
            .await
            .unwrap(),
        Handed::Queued
    );
    assert_eq!(
        texts(claims, &ids, std::slice::from_ref(&t), 10).await,
        ["x"]
    );
}

async fn an_expired_claim_takes_no_message(claims: &dyn TurnClaimStore) {
    let ids = fresh();
    let t = token();
    assert!(
        claims
            .claim("", &ids.task, &ids.owner, &t, Duration::from_millis(30))
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        claims
            .hand_steering("", &ids.task, "late", 10)
            .await
            .unwrap(),
        Handed::NoClaim
    );
    assert_eq!(texts(claims, &ids, &[t], 10).await, Vec::<String>::new());
}

/// A message handed to one turn is never read by a later turn of the task:
/// not after the claim is released, and not after another replica took the
/// task over from an expired claim.
async fn a_message_never_reaches_a_later_claim(claims: &dyn TurnClaimStore) {
    // Released.
    let ids = fresh();
    let first = claimed(claims, "", &ids).await;
    assert_eq!(
        claims
            .hand_steering("", &ids.task, "old", 10)
            .await
            .unwrap(),
        Handed::Queued
    );
    claims.release(&first).await.unwrap();
    let second = claimed(claims, "", &ids).await;
    assert!(
        texts(claims, &ids, &[first.clone(), second.clone()], 10)
            .await
            .is_empty(),
        "the next turn does not read the last one's mail"
    );

    // Taken over after the claim expired.
    let ids = fresh();
    let old = token();
    assert!(
        claims
            .claim("", &ids.task, &ids.owner, &old, Duration::from_millis(30))
            .await
            .unwrap()
    );
    assert_eq!(
        claims
            .hand_steering("", &ids.task, "older", 10)
            .await
            .unwrap(),
        Handed::Queued
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let new = claimed(claims, "", &ids).await;
    assert!(
        texts(claims, &ids, &[old, new], 10).await.is_empty(),
        "a taken-over claim's mail does not go to the one that took it"
    );
}

/// Twelve senders at once, a capacity of three: exactly three are stored.
async fn the_bound_holds_under_concurrent_senders(claims: Arc<dyn TurnClaimStore>) {
    let ids = fresh();
    let t = claimed(&*claims, "", &ids).await;
    let senders: Vec<_> = (0..12)
        .map(|i| {
            let claims = Arc::clone(&claims);
            let task = ids.task.clone();
            tokio::spawn(async move {
                claims
                    .hand_steering("", &task, &format!("m{i}"), 3)
                    .await
                    .unwrap()
            })
        })
        .collect();
    let mut queued = 0;
    let mut full = 0;
    for sender in senders {
        match sender.await.unwrap() {
            Handed::Queued => queued += 1,
            Handed::Full => full += 1,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!((queued, full), (3, 9));
    assert_eq!(texts(&*claims, &ids, &[t], 100).await.len(), 3);
}

pub(super) async fn every_case(claims: Arc<dyn TurnClaimStore>) {
    nothing_is_stored_for_a_task_nobody_holds(&*claims).await;
    a_message_waits_for_the_claims_owner_in_order(&*claims).await;
    the_queue_is_bounded_by_the_capacity(&*claims).await;
    a_message_is_for_the_scope_of_the_claim(&*claims).await;
    an_expired_claim_takes_no_message(&*claims).await;
    a_message_never_reaches_a_later_claim(&*claims).await;
    the_bound_holds_under_concurrent_senders(claims).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_in_memory_store_hands_steering_over() {
    every_case(Arc::new(super::InMemoryTurnClaims::new())).await;
}

/// Releasing a claim deletes what was handed to it, and only that: nothing
/// is left to read from a store's memory for a turn that ended.
#[tokio::test]
async fn releasing_a_claim_deletes_its_messages_and_only_its_own() {
    let claims = super::InMemoryTurnClaims::new();
    let (gone, kept) = (fresh(), fresh());
    let gone_token = claimed(&claims, "", &gone).await;
    let kept_token = claimed(&claims, "", &kept).await;
    for ids in [&gone, &kept] {
        assert_eq!(
            claims.hand_steering("", &ids.task, "x", 10).await.unwrap(),
            Handed::Queued
        );
    }
    assert_eq!(claims.queued(), 2);
    claims.release(&gone_token).await.unwrap();
    assert_eq!(claims.queued(), 1, "the released claim's message is gone");
    assert_eq!(
        texts(&claims, &kept, &[kept_token], 10).await,
        ["x"],
        "the other claim's message is still there to read"
    );
}

/// A store that does not say it can hand a message over refuses to, and
/// finds none: what a claim store written before the extension does.
#[tokio::test]
async fn a_store_that_says_nothing_hands_nothing_over() {
    struct Before(super::InMemoryTurnClaims);
    impl TurnClaimStore for Before {
        fn claim<'a>(
            &'a self,
            scope: &'a str,
            task_id: &'a str,
            owner: &'a str,
            token: &'a str,
            ttl: Duration,
        ) -> super::Fut<'a, bool> {
            self.0.claim(scope, task_id, owner, token, ttl)
        }
        fn release<'a>(&'a self, token: &'a str) -> super::Fut<'a, ()> {
            self.0.release(token)
        }
        fn renew<'a>(
            &'a self,
            owner: &'a str,
            tokens: &'a [String],
            ttl: Duration,
        ) -> super::Fut<'a, Vec<super::HeldClaim>> {
            self.0.renew(owner, tokens, ttl)
        }
        fn request_cancel<'a>(&'a self, scope: &'a str, task_id: &'a str) -> super::Fut<'a, bool> {
            self.0.request_cancel(scope, task_id)
        }
        fn sweep(&self, older_than: Duration) -> super::Fut<'_, u64> {
            self.0.sweep(older_than)
        }
    }
    let store = Before(super::InMemoryTurnClaims::new());
    let ids = fresh();
    let t = claimed(&store, "", &ids).await;
    assert_eq!(
        store.hand_steering("", &ids.task, "x", 10).await.unwrap(),
        Handed::Unsupported
    );
    assert_eq!(texts(&store, &ids, &[t], 10).await, Vec::<String>::new());
    store.ack_steering(&[1, 2]).await.unwrap();
    assert!(store.steering_signals().is_none());
}
