// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A call that needs approval waits in the journal for a decision on exactly that
//! call (adversarial review S7, 2026-11-12).

use super::journal::{KEY, journal, looped, model_fails, one_transfer, run_keyed};
use super::strict::{identity, mutating};
use super::*;
use crate::tool_loop::journal::StepJournal;
use crate::tool_loop::journaling::Verdict;
use crate::tool_loop::policy::{PolicyDecision, PolicyRequest, ToolPolicy, call_digest};
use crate::tool_loop::steering::Side;
use serde_json::json;

/// Needs approval for `transfer`, denies `blocked`, allows the rest.
pub(super) struct NeedsApproval;

impl ToolPolicy for NeedsApproval {
    fn authorize<'a>(
        &'a self,
        request: &'a PolicyRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = PolicyDecision> + Send + 'a>> {
        let decision = match request.tool {
            "transfer" => PolicyDecision::RequireApproval("transfer money".to_owned()),
            "blocked" => PolicyDecision::Deny("never".to_owned()),
            _ => PolicyDecision::Allow,
        };
        Box::pin(async move { decision })
    }
}

pub(super) fn alice() -> crate::tool_loop::TrustedIdentity {
    identity("alice")
}

pub(super) fn bob() -> crate::tool_loop::TrustedIdentity {
    identity("bob")
}

pub(super) fn verdict_for(
    call_args: &Value,
    who: &crate::tool_loop::TrustedIdentity,
    approved: bool,
) -> Verdict {
    Verdict {
        digest: call_digest(
            "transfer",
            call_args,
            who.tenant.as_deref(),
            who.principal.as_deref(),
            who.task.as_deref(),
        ),
        approved,
        by: "bob".to_owned(),
    }
}

/// Parks, then resumes on a decision for that exact call; the call is made
/// from the journal, not from anything the decision carries.
#[tokio::test]
async fn a_call_that_needs_approval_waits_and_then_runs_exactly_once() {
    let j = journal();
    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer()]);
    let tool_loop = looped(model, vec![transfer], &j).with_policy(NeedsApproval);
    let who = alice();
    let parked = run_keyed(
        &tool_loop,
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await;
    let Err(LoopError::AwaitingApproval(needed)) = parked else {
        panic!("expected it to wait: {parked:?}");
    };
    assert_eq!(
        used.load(Ordering::SeqCst),
        0,
        "it ran before it was approved"
    );
    assert_eq!(needed.request_id, KEY);
    assert_eq!(needed.tool, "transfer");
    let args = json!({"service": "x"});
    assert_eq!(needed.digest, verdict_for(&args, &who, true).digest);
    let stored = j.load(KEY).await.unwrap().unwrap().checkpoint().unwrap();
    assert_eq!(
        stored.pending_approval.as_ref().map(|p| p.digest.clone()),
        Some(needed.digest.clone())
    );
    assert_eq!(stored.receipts[0].state, ReceiptState::AwaitingApproval);

    // Bob, another principal, approves; the run is Alice's.
    let (transfer, used) = mutating();
    let (model, seen) = Scripted::new(vec![text("sent")]);
    let tool_loop = looped(model, vec![transfer], &j).with_policy(NeedsApproval);
    let decision = verdict_for(&args, &who, true);
    let answer = run_keyed(
        &tool_loop,
        &needed.request_id,
        Side {
            identity: Some(&bob()),
            approval: Some(&decision),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(answer.text, "sent");
    assert_eq!(used.load(Ordering::SeqCst), 1, "the approved call ran once");
    assert_eq!(results(&seen.lock().unwrap().clone(), 0).len(), 1);
    assert_eq!((j.len(), j.unfinished()), (1, 0), "kept, completed");
}

#[tokio::test]
async fn a_refusal_is_a_result_the_model_sees_and_the_call_never_runs() {
    let j = journal();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer()]);
    let who = alice();
    let _ = run_keyed(
        &looped(model, vec![transfer], &j).with_policy(NeedsApproval),
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await;
    let (transfer, used) = mutating();
    let (model, seen) = Scripted::new(vec![text("understood")]);
    let no = verdict_for(&json!({"service": "x"}), &who, false);
    let answer = run_keyed(
        &looped(model, vec![transfer], &j).with_policy(NeedsApproval),
        KEY,
        Side {
            identity: Some(&bob()),
            approval: Some(&no),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(answer.text, "understood");
    assert_eq!(used.load(Ordering::SeqCst), 0);
    let told = results(&seen.lock().unwrap().clone(), 0);
    assert!(told[0].1.contains("was not approved by bob"), "{told:?}");
}

/// An approval releases the call it was given for and nothing else.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn an_approval_for_another_call_requester_or_tenant_releases_nothing() {
    let j = journal();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer()]);
    let who = alice();
    let _ = run_keyed(
        &looped(model, vec![transfer], &j).with_policy(NeedsApproval),
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await;

    let resume = |decision: Verdict, by: crate::tool_loop::TrustedIdentity| {
        let j = Arc::clone(&j);
        async move {
            let (transfer, used) = mutating();
            let (model, _) = Scripted::new(vec![text("done")]);
            let r = run_keyed(
                &looped(model, vec![transfer], &j).with_policy(NeedsApproval),
                KEY,
                Side {
                    identity: Some(&by),
                    approval: Some(&decision),
                    ..Side::default()
                },
            )
            .await;
            (r, used.load(Ordering::SeqCst))
        }
    };
    // Other arguments.
    let (r, ran) = resume(verdict_for(&json!({"service": "y"}), &who, true), bob()).await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("different action")),
        "{r:?}"
    );
    assert_eq!(ran, 0);
    // The same call as another requester's.
    let (r, ran) = resume(verdict_for(&json!({"service": "x"}), &bob(), true), bob()).await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("different action")),
        "{r:?}"
    );
    assert_eq!(ran, 0);
    // The right digest from another tenant.
    let mut elsewhere = bob();
    elsewhere.tenant = Some("globex".to_owned());
    let (r, ran) = resume(verdict_for(&json!({"service": "x"}), &who, true), elsewhere).await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("another tenant or task")),
        "{r:?}"
    );
    assert_eq!(ran, 0);
    // The right digest from another task.
    let mut other_task = bob();
    other_task.task = Some("task-9".to_owned());
    let (r, ran) = resume(
        verdict_for(&json!({"service": "x"}), &who, true),
        other_task,
    )
    .await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("another tenant or task")),
        "{r:?}"
    );
    assert_eq!(ran, 0);
    // No run waits under another key.
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![text("done")]);
    let r = run_keyed(
        &looped(model, vec![transfer], &j).with_policy(NeedsApproval),
        "no-such-run",
        Side {
            identity: Some(&bob()),
            approval: Some(&verdict_for(&json!({"service": "x"}), &who, true)),
            ..Side::default()
        },
    )
    .await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("no run is waiting")),
        "{r:?}"
    );
}

/// The policy is asked again on resume: an approval does not outrank a denial.
#[tokio::test]
async fn an_approval_does_not_outrank_a_denial_on_resume() {
    struct Then(AtomicUsize);
    impl ToolPolicy for Then {
        fn authorize<'a>(
            &'a self,
            _request: &'a PolicyRequest<'a>,
        ) -> Pin<Box<dyn Future<Output = PolicyDecision> + Send + 'a>> {
            let first = self.0.fetch_add(1, Ordering::SeqCst) == 0;
            Box::pin(async move {
                if first {
                    PolicyDecision::RequireApproval("transfer".to_owned())
                } else {
                    PolicyDecision::Deny("policy changed".to_owned())
                }
            })
        }
    }
    let j = journal();
    let who = alice();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer()]);
    let _ = run_keyed(
        &looped(model, vec![transfer], &j).with_policy(Then(AtomicUsize::new(0))),
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await;
    let (transfer, used) = mutating();
    let (model, seen) = Scripted::new(vec![text("ok")]);
    let yes = verdict_for(&json!({"service": "x"}), &who, true);
    run_keyed(
        &looped(model, vec![transfer], &j).with_policy(Then(AtomicUsize::new(1))),
        KEY,
        Side {
            identity: Some(&bob()),
            approval: Some(&yes),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 0);
    assert!(
        results(&seen.lock().unwrap().clone(), 0)[0]
            .1
            .contains("policy changed")
    );
}

/// With no journal there is nowhere to wait: the call is refused, not run.
#[tokio::test]
async fn a_call_that_needs_approval_is_refused_where_there_is_no_journal() {
    let (transfer, used) = mutating();
    let (model, seen) = Scripted::new(vec![one_transfer(), text("ok")]);
    let tool_loop = ToolLoop::new(model, tools(vec![transfer])).with_policy(NeedsApproval);
    let who = alice();
    run_keyed(
        &tool_loop,
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 0);
    let told = results(&seen.lock().unwrap().clone(), 1);
    assert!(told[0].1.contains("no journal to wait in"), "{told:?}");
}

/// A decision for a loop that keeps no journal finds no run.
#[tokio::test]
async fn a_decision_for_a_loop_without_a_journal_finds_no_run() {
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![text("ok")]);
    let who = alice();
    let yes = verdict_for(&json!({"service": "x"}), &who, true);
    let r = run_keyed(
        &ToolLoop::new(model, tools(vec![transfer])),
        KEY,
        Side {
            identity: Some(&bob()),
            approval: Some(&yes),
            ..Side::default()
        },
    )
    .await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("no journal")),
        "{r:?}"
    );
}

/// A reply with two calls, the first needing approval: the second is not run
/// ahead of it, the run parks on the first, and once that is decided the
/// checkpoint no longer says a call is pending.
#[tokio::test]
async fn a_decided_call_is_no_longer_pending_in_the_checkpoint() {
    let j = journal();
    let who = alice();
    let (transfer, _) = mutating();
    let (lookup, looked) = TestTool::new("lookup", |_| Ok("found".to_owned()));
    let two = calls(vec![
        call("c1", "transfer", r#"{"service":"x"}"#),
        call("c2", "lookup", r#"{"service":"y"}"#),
    ]);
    let (model, _) = Scripted::new(vec![two]);
    let r = run_keyed(
        &looped(model, vec![transfer, lookup], &j).with_policy(NeedsApproval),
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await;
    assert!(matches!(r, Err(LoopError::AwaitingApproval(_))), "{r:?}");
    assert_eq!(
        looked.load(Ordering::SeqCst),
        0,
        "a later call ran before the one waiting"
    );

    // Approved, and then the model dies before it answers: the checkpoint
    // records both calls as answered and nothing as pending.
    let (transfer, _) = mutating();
    let (lookup, _) = TestTool::new("lookup", |_| Ok("found".to_owned()));
    let (model, _) = Scripted::new(vec![model_fails()]);
    let yes = verdict_for(&json!({"service": "x"}), &who, true);
    let r = run_keyed(
        &looped(model, vec![transfer, lookup], &j).with_policy(NeedsApproval),
        KEY,
        Side {
            identity: Some(&bob()),
            approval: Some(&yes),
            ..Side::default()
        },
    )
    .await;
    assert!(matches!(r, Err(LoopError::Model(_))), "{r:?}");
    let stored = j.load(KEY).await.unwrap().unwrap().checkpoint().unwrap();
    assert_eq!(stored.pending_approval, None);
    assert_eq!(
        stored.receipts.iter().map(|r| r.state).collect::<Vec<_>>(),
        [ReceiptState::Ran, ReceiptState::Ran]
    );
}
