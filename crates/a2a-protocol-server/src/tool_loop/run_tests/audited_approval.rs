// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An approved call is on the audit record as approved before it runs, and
//! does not run if that record cannot be written (adversarial review N02,
//! 2026-11-13).
//!
//! Until then `AuditedPolicy` recorded a call sent for approval as `deny`,
//! ignored whether that record was written, and the loop ran the call once a
//! person approved it with no record of it running: with the audit store down
//! after the run was parked, an approved transfer ran unrecorded.

use super::journal::{KEY, journal, looped, one_transfer, run_keyed};
use super::journal_approval::{NeedsApproval, alice, bob, verdict_for};
use super::strict::mutating;
use super::*;
use crate::audit::tests::outcome_durability::Flaky;
use crate::audit::{AuditLog, AuditStore};
use crate::tool_loop::AuditedPolicy;
use crate::tool_loop::policy::{PolicyDecision, PolicyRequest, ToolPolicy};
use crate::tool_loop::steering::Side;
use serde_json::json;
use std::sync::Mutex as StdMutex;

fn required_log(store: &Arc<Flaky>) -> Arc<AuditLog> {
    Arc::new(AuditLog::new(Arc::clone(store) as Arc<dyn AuditStore>).require_record(true))
}

/// Parks Alice's transfer for approval, under an audited policy over `log`.
async fn park(log: &Arc<AuditLog>, j: &Arc<crate::tool_loop::InMemoryStepJournal>) {
    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer()]);
    let tool_loop = looped(model, vec![transfer], j)
        .with_policy(AuditedPolicy::new(Arc::clone(log)).wrapping(NeedsApproval));
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
    assert!(
        matches!(parked, Err(LoopError::AwaitingApproval(_))),
        "{parked:?}"
    );
    assert_eq!(used.load(Ordering::SeqCst), 0);
}

/// The decisions of the `tool.action` records, in order.
async fn decisions(log: &AuditLog) -> Vec<String> {
    log.export("")
        .await
        .unwrap()
        .iter()
        .filter(|r| r.kind == "tool.action")
        .map(|r| r.detail["decision"].as_str().unwrap().to_owned())
        .collect()
}

/// The store goes down after the run is parked and before the approved call
/// runs, the journal stays healthy: the call is not run, and the model is told
/// why.
#[tokio::test]
async fn an_approved_call_whose_record_cannot_be_written_does_not_run() {
    let store = Flaky::new();
    let log = required_log(&store);
    let j = journal();
    park(&log, &j).await;

    store.set_down(true);
    let (transfer, used) = mutating();
    let (model, seen) = Scripted::new(vec![text("could not")]);
    let tool_loop = looped(model, vec![transfer], &j)
        .with_policy(AuditedPolicy::new(Arc::clone(&log)).wrapping(NeedsApproval));
    let decision = verdict_for(&json!({"service": "x"}), &alice(), true);
    let answer = run_keyed(
        &tool_loop,
        KEY,
        Side {
            identity: Some(&bob()),
            approval: Some(&decision),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        used.load(Ordering::SeqCst),
        0,
        "an approved action ran with no record of running"
    );
    assert_eq!(answer.text, "could not");
    let told = results(&seen.lock().unwrap().clone(), 0);
    assert!(told[0].1.contains("could not be recorded"), "{told:?}");
    assert!(log.failures() >= 1);
}

/// With a healthy store, the record says what happened: the call waited
/// (`approval_required`, once parked and once more when the policy is asked
/// again on resume), then ran approved, naming the approver; nothing says
/// `deny`. The approved record is in the store when the tool begins.
#[tokio::test]
async fn an_approved_call_is_recorded_as_approved_before_it_runs() {
    let store = Flaky::new();
    let log = required_log(&store);
    let j = journal();
    park(&log, &j).await;
    assert_eq!(decisions(&log).await, vec!["approval_required"]);

    let seen_by_the_tool = Arc::new(StdMutex::new(None));
    let (transfer, used) = TestTool::new("transfer", {
        let store = Arc::clone(&store);
        let seen = Arc::clone(&seen_by_the_tool);
        move |_| {
            *seen.lock().unwrap() = Some(store.stored.load(Ordering::SeqCst));
            Ok("sent".to_owned())
        }
    });
    let (model, _) = Scripted::new(vec![text("sent")]);
    let tool_loop = looped(model, vec![transfer], &j)
        .with_policy(AuditedPolicy::new(Arc::clone(&log)).wrapping(NeedsApproval));
    let decision = verdict_for(&json!({"service": "x"}), &alice(), true);
    run_keyed(
        &tool_loop,
        KEY,
        Side {
            identity: Some(&bob()),
            approval: Some(&decision),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 1);
    assert_eq!(
        decisions(&log).await,
        vec!["approval_required", "approval_required", "approved"]
    );
    assert_eq!(
        *seen_by_the_tool.lock().unwrap(),
        Some(3),
        "the approved record was not in the store when the tool began"
    );
    let approved = log.export("").await.unwrap().pop().unwrap();
    assert_eq!(approved.detail["approvedBy"], json!(["bob"]));
    assert_eq!(approved.detail["toolCallId"], "c1");
    assert!(log.verify("").await.unwrap().is_intact());
}

/// A person's refusal runs nothing and asks the hook nothing: there is no
/// approved record.
#[tokio::test]
async fn a_refused_call_has_no_approved_record() {
    let store = Flaky::new();
    let log = required_log(&store);
    let j = journal();
    park(&log, &j).await;
    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![text("understood")]);
    let tool_loop = looped(model, vec![transfer], &j)
        .with_policy(AuditedPolicy::new(Arc::clone(&log)).wrapping(NeedsApproval));
    let no = verdict_for(&json!({"service": "x"}), &alice(), false);
    run_keyed(
        &tool_loop,
        KEY,
        Side {
            identity: Some(&bob()),
            approval: Some(&no),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 0);
    assert!(!decisions(&log).await.contains(&"approved".to_owned()));
}

/// The wrapped policy's own hook is asked first, and its refusal stops the
/// call before anything is recorded as approved.
#[tokio::test]
async fn the_wrapped_policys_refusal_of_an_approved_call_is_final() {
    struct Vetoes;
    impl ToolPolicy for Vetoes {
        fn authorize<'a>(
            &'a self,
            _request: &'a PolicyRequest<'a>,
        ) -> Pin<Box<dyn Future<Output = PolicyDecision> + Send + 'a>> {
            Box::pin(async { PolicyDecision::RequireApproval("transfer money".to_owned()) })
        }
        fn approved<'a>(
            &'a self,
            _request: &'a PolicyRequest<'a>,
            approver: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move { Err(format!("{approver} may not approve transfers")) })
        }
    }
    let store = Flaky::new();
    let log = required_log(&store);
    let j = journal();
    {
        let (transfer, _) = mutating();
        let (model, _) = Scripted::new(vec![one_transfer()]);
        let tool_loop = looped(model, vec![transfer], &j)
            .with_policy(AuditedPolicy::new(Arc::clone(&log)).wrapping(Vetoes));
        let who = alice();
        let _ = run_keyed(
            &tool_loop,
            KEY,
            Side {
                identity: Some(&who),
                ..Side::default()
            },
        )
        .await;
    }
    let (transfer, used) = mutating();
    let (model, seen) = Scripted::new(vec![text("ok")]);
    let tool_loop = looped(model, vec![transfer], &j)
        .with_policy(AuditedPolicy::new(Arc::clone(&log)).wrapping(Vetoes));
    let decision = verdict_for(&json!({"service": "x"}), &alice(), true);
    run_keyed(
        &tool_loop,
        KEY,
        Side {
            identity: Some(&bob()),
            approval: Some(&decision),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(used.load(Ordering::SeqCst), 0);
    let told = results(&seen.lock().unwrap().clone(), 0);
    assert!(told[0].1.contains("bob may not approve"), "{told:?}");
    assert!(!decisions(&log).await.contains(&"approved".to_owned()));
}
