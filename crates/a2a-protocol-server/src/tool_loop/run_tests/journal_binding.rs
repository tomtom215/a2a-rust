// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What an approval is bound to, and how it behaves across calls and deaths
//! (adversarial review S7, 2026-11-12): the digest covers tool, arguments,
//! requester, tenant and task; an approval releases the one call it names.

use super::journal::{Blocks, KEY, blocking, journal, looped, one_transfer, run_keyed};
use super::journal_approval::{NeedsApproval, alice, bob, verdict_for};
use super::strict::mutating;
use super::*;
use crate::tool_loop::journal::{InMemoryStepJournal, ReceiptState, StepJournal};
use crate::tool_loop::policy::call_digest;
use crate::tool_loop::steering::Side;
use serde_json::json;

/// The digest is bound to who and where, and is stable under key order.
#[test]
#[allow(clippy::too_many_lines)]
fn the_call_digest_binds_tool_arguments_requester_tenant_and_task() {
    let d = |tool: &str, a: &Value, t: Option<&str>, p: Option<&str>, k: Option<&str>| {
        call_digest(tool, a, t, p, k)
    };
    let base = d(
        "transfer",
        &json!({"a": 1, "b": 2}),
        Some("acme"),
        Some("alice"),
        Some("t"),
    );
    assert!(base.starts_with("sha256:") && base.len() == 7 + 64);
    assert_eq!(
        base,
        d(
            "transfer",
            &json!({"b": 2, "a": 1}),
            Some("acme"),
            Some("alice"),
            Some("t")
        )
    );
    for other in [
        d(
            "refund",
            &json!({"a": 1, "b": 2}),
            Some("acme"),
            Some("alice"),
            Some("t"),
        ),
        d(
            "transfer",
            &json!({"a": 1, "b": 3}),
            Some("acme"),
            Some("alice"),
            Some("t"),
        ),
        d(
            "transfer",
            &json!({"a": 1, "b": 2}),
            Some("globex"),
            Some("alice"),
            Some("t"),
        ),
        d(
            "transfer",
            &json!({"a": 1, "b": 2}),
            Some("acme"),
            Some("bob"),
            Some("t"),
        ),
        d(
            "transfer",
            &json!({"a": 1, "b": 2}),
            Some("acme"),
            Some("alice"),
            Some("u"),
        ),
        d(
            "transfer",
            &json!({"a": 1, "b": 2}),
            None,
            Some("alice"),
            Some("t"),
        ),
    ] {
        assert_ne!(base, other);
    }
}

/// A loop over `tool` that needs approval for `transfer` and has no shared
/// idempotency secret, so its keys differ from one replica to the next.
fn approving(model: Scripted, tool: Blocks, j: &Arc<InMemoryStepJournal>) -> ToolLoop {
    let mut set = ToolSet::new();
    set.add(tool).unwrap();
    ToolLoop::new(model, set)
        .with_journal(Arc::clone(j) as Arc<dyn StepJournal>)
        .with_policy(NeedsApproval)
}

/// An approved call that dies in flight is not re-run under a key that differs
/// between replicas: its receipt says `Started` from the moment it begins, even
/// though it was waiting for approval a moment before.
#[tokio::test]
async fn an_approved_call_that_died_in_flight_is_indeterminate_not_rerun() {
    let j = journal();
    let who = alice();
    let keys = Arc::new(Mutex::new(Vec::new()));
    let (tool, _, _) = blocking(&keys, false);
    let (model, _) = Scripted::new(vec![one_transfer()]);
    let r = run_keyed(
        &approving(model, tool, &j),
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await;
    assert!(matches!(r, Err(LoopError::AwaitingApproval(_))), "{r:?}");

    // Approved; the process dies inside the call.
    let (tool, entered, _) = blocking(&keys, true);
    let (model, _) = Scripted::new(vec![text("never")]);
    let yes = verdict_for(&json!({"service": "x"}), &who, true);
    let looped_again = approving(model, tool, &j);
    let approver = bob();
    let mut run = Box::pin(run_keyed(
        &looped_again,
        KEY,
        Side {
            identity: Some(&approver),
            approval: Some(&yes),
            ..Side::default()
        },
    ));
    tokio::select! {
        r = &mut run => panic!("the run ended: {r:?}"),
        () = entered.notified() => {}
    }
    drop(run);
    let stored = j.load(KEY).await.unwrap().unwrap();
    assert_eq!(
        stored.receipts.iter().map(|r| r.state).collect::<Vec<_>>(),
        [ReceiptState::Started],
        "the checkpoint written before the call says it may have run"
    );

    // Resumed on a replica whose keys would differ: refused, not guessed.
    let (tool, _, calls) = blocking(&keys, false);
    let (model, _) = Scripted::new(vec![text("never")]);
    let r = run_keyed(
        &approving(model, tool, &j),
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await;
    assert!(matches!(r, Err(LoopError::Indeterminate { .. })), "{r:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

/// An approval releases the call it was given for and not a second call of the
/// same turn that needs approval of its own.
#[tokio::test]
async fn an_approval_does_not_release_a_second_call_that_needs_its_own() {
    let j = journal();
    let who = alice();
    let two = || {
        calls(vec![
            call("c1", "transfer", r#"{"service":"x"}"#),
            call("c2", "transfer", r#"{"service":"y"}"#),
        ])
    };
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![two()]);
    let r = run_keyed(
        &looped(model, vec![transfer], &j).with_policy(NeedsApproval),
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await;
    assert!(matches!(r, Err(LoopError::AwaitingApproval(_))), "{r:?}");

    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![text("never")]);
    let yes = verdict_for(&json!({"service": "x"}), &who, true);
    let r = run_keyed(
        &looped(model, vec![transfer], &j).with_policy(NeedsApproval),
        KEY,
        Side {
            identity: Some(&bob()),
            approval: Some(&yes),
            ..Side::default()
        },
    )
    .await;
    let Err(LoopError::AwaitingApproval(second)) = r else {
        panic!("the second call should wait for its own approval: {r:?}");
    };
    assert_eq!(second.call_id, "c2");
    assert_eq!(used.load(Ordering::SeqCst), 1, "only the approved call ran");
}
