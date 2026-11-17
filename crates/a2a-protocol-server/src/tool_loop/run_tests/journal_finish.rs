// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How a journaled run ends (adversarial review N04, 2026-11-13): its answer is
//! a completed checkpoint, written under the revision fence, and replayed;
//! a stale invocation's answer cannot remove a newer one's progress.
//!
//! Until then a run cleared its checkpoint on a final answer, unfenced and
//! before the executor published the answer: a stale invocation's answer
//! deleted a newer invocation's checkpoint, and a process that died between
//! the clear and the publication lost the conversation, the receipts and the
//! answer, so a replay began again from nothing.

use tokio::sync::Notify;

use super::journal::{KEY, journal, looped, model_fails, one_transfer, run_keyed};
use super::strict::{identity, mutating};
use super::*;
use crate::tool_loop::journal::StepJournal;
use crate::tool_loop::steering::Side;

/// Answers `reply` once `go` is notified, after saying so on `asked`.
struct Gated {
    reply: Mutex<Option<Result<ModelTurn, ModelError>>>,
    asked: Arc<Notify>,
    go: Arc<Notify>,
}

impl ToolModel for Gated {
    fn model_name(&self) -> &'static str {
        "scripted"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(async move {
            self.asked.notify_one();
            self.go.notified().await;
            self.reply
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Err(ModelError::Other("asked twice".to_owned())))
        })
    }
}

/// A run with one transfer done, whose next step is a model turn.
async fn one_call_done(j: &Arc<crate::tool_loop::InMemoryStepJournal>) -> u64 {
    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer(), model_fails()]);
    let r = run_keyed(&looped(model, vec![transfer], j), KEY, Side::default()).await;
    assert!(matches!(r, Err(LoopError::Model(_))), "{r:?}");
    assert_eq!(used.load(Ordering::SeqCst), 1);
    j.load(KEY).await.unwrap().unwrap().revision
}

/// A, slow, answers after B has moved the same run on: A's answer is refused
/// as stale, and B's checkpoint is still there.
#[tokio::test]
async fn a_stale_answer_cannot_remove_a_newer_runs_checkpoint() {
    let store = journal();
    let start = one_call_done(&store).await;

    let (asked, go) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let (transfer_a, used_a) = mutating();
    let slow = ToolLoop::new(
        Gated {
            reply: Mutex::new(Some(text("A's answer"))),
            asked: Arc::clone(&asked),
            go: Arc::clone(&go),
        },
        tools(vec![transfer_a]),
    )
    .with_journal(Arc::clone(&store) as Arc<dyn StepJournal>)
    .with_idempotency_secret([7; 32]);
    let stale = tokio::spawn(async move { run_keyed(&slow, KEY, Side::default()).await });
    asked.notified().await;

    // B: asks for and runs a second transfer, then its model fails.
    let (transfer_b, used_b) = mutating();
    let (model, _) = Scripted::new(vec![
        calls(vec![call("c2", "transfer", r#"{"service":"y"}"#)]),
        model_fails(),
    ]);
    let newer = run_keyed(
        &looped(model, vec![transfer_b], &store),
        KEY,
        Side::default(),
    )
    .await;
    assert!(matches!(newer, Err(LoopError::Model(_))), "{newer:?}");
    assert_eq!(used_b.load(Ordering::SeqCst), 1);
    let moved_on = store.load(KEY).await.unwrap().unwrap();
    assert!(moved_on.revision > start);

    go.notify_one();
    let stale = stale.await.unwrap();
    assert!(
        matches!(&stale, Err(LoopError::Journal(w)) if w.contains("another writer")),
        "a stale invocation published an answer: {stale:?}"
    );
    assert_eq!(used_a.load(Ordering::SeqCst), 0);
    assert_eq!(
        store.load(KEY).await.unwrap(),
        Some(moved_on),
        "the newer invocation's checkpoint was removed or changed"
    );
}

/// The process dies after the answer is journaled and before the task is
/// published: a replay gives the same answer, asks no model and runs no tool.
#[tokio::test]
async fn a_finished_run_replays_its_answer_and_does_nothing_else() {
    let j = journal();
    one_call_done(&j).await;
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![text("sent, ref 4471")]);
    let who = identity("alice");
    let first = run_keyed(
        &looped(model, vec![transfer], &j),
        KEY,
        Side {
            identity: Some(&who),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    let kept = j.load(KEY).await.unwrap().unwrap();
    assert!(kept.completed);

    // "Restart": a new loop, whose model and tool must not be used.
    let (transfer, used) = mutating();
    let (model, seen) = Scripted::new(vec![model_fails()]);
    let again = run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default())
        .await
        .unwrap();
    assert_eq!(again.text, first.text);
    assert_eq!(again.trace, first.trace, "the receipts' trace is kept");
    assert_eq!(again.finish_reason, first.finish_reason);
    assert_eq!(seen.lock().unwrap().len(), 0, "the model was asked again");
    assert_eq!(used.load(Ordering::SeqCst), 0, "a tool ran again");
    assert_eq!(
        j.load(KEY).await.unwrap(),
        Some(kept),
        "a replay writes nothing"
    );
}

/// An approval for a run that has finished is refused, not applied.
#[tokio::test]
async fn an_approval_for_a_finished_run_is_refused() {
    let j = journal();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![text("done")]);
    run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default())
        .await
        .unwrap();
    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![text("again")]);
    let verdict = crate::tool_loop::journaling::Verdict {
        digest: "sha256:00".to_owned(),
        approved: true,
        by: "bob".to_owned(),
    };
    let r = run_keyed(
        &looped(model, vec![transfer], &j),
        KEY,
        Side {
            approval: Some(&verdict),
            ..Side::default()
        },
    )
    .await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("has finished")),
        "{r:?}"
    );
    assert_eq!(used.load(Ordering::SeqCst), 0);
}

/// Whose run it is comes from the request, and stays with the run when
/// another identity (an approver) resumes it.
#[tokio::test]
async fn a_run_is_found_and_erased_by_its_owner() {
    let j = journal();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer(), model_fails()]);
    let alice = identity("alice");
    let _ = run_keyed(
        &looped(model, vec![transfer], &j),
        KEY,
        Side {
            identity: Some(&alice),
            ..Side::default()
        },
    )
    .await;
    let runs = j.runs_of("alice", alice.tenant.as_deref()).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].key, KEY);
    assert_eq!(runs[0].owner.task, alice.task);
    assert!(!runs[0].completed);
    assert_eq!(j.runs_of("bob", None).await.unwrap().len(), 0);

    // Bob resumes it (as an approver would); it stays Alice's.
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![text("done")]);
    let bob = identity("bob");
    run_keyed(
        &looped(model, vec![transfer], &j),
        KEY,
        Side {
            identity: Some(&bob),
            ..Side::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(j.runs_of("alice", None).await.unwrap().len(), 1);
    assert_eq!(j.runs_of("bob", None).await.unwrap().len(), 0);
    assert_eq!(j.erase("alice", None).await.unwrap(), 1);
    assert!(j.is_empty());
}
