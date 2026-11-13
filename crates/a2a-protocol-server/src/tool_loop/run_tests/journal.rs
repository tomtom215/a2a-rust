// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The step journal and approvals bound to a call (adversarial review S1, S7,
//! 2026-11-12): a run begun again resumes, what it was begun with is pinned, a
//! call is recorded before it runs, and a call that needs approval waits for a
//! decision on exactly that call.

use super::strict::mutating;
use super::*;
use crate::tool_loop::journal::{InMemoryStepJournal, ReceiptState, StepJournal};
use crate::tool_loop::steering::Side;
use serde_json::json;

pub(super) const KEY: &str = "run-1";

pub(super) fn journal() -> Arc<InMemoryStepJournal> {
    Arc::new(InMemoryStepJournal::new())
}

pub(super) fn looped(
    model: Scripted,
    list: Vec<TestTool>,
    j: &Arc<InMemoryStepJournal>,
) -> ToolLoop {
    ToolLoop::new(model, tools(list))
        .with_journal(Arc::clone(j) as Arc<dyn StepJournal>)
        .with_idempotency_secret([7; 32])
}

pub(super) async fn run_keyed(
    tool_loop: &ToolLoop,
    key: &str,
    side: Side<'_>,
) -> Result<Answer, LoopError> {
    tool_loop
        .run_drafting(key, "go", &CancellationToken::new(), None, side)
        .await
}

pub(super) fn one_transfer() -> Result<ModelTurn, ModelError> {
    calls(vec![call("c1", "transfer", r#"{"service":"x"}"#)])
}

pub(super) fn model_fails() -> Result<ModelTurn, ModelError> {
    Err(ModelError::Other("the replica died".to_owned()))
}

// ── S1: resume ───────────────────────────────────────────────────────────────

/// A run that dies after a tool ran is resumed by the next attempt under the
/// same key: the tool is not run again and the model is not asked again for
/// what it already answered.
#[tokio::test]
async fn a_run_begun_again_resumes_and_does_not_repeat_a_call() {
    let j = journal();
    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer(), model_fails()]);
    let first = run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default()).await;
    assert!(matches!(first, Err(LoopError::Model(_))), "{first:?}");
    assert_eq!(used.load(Ordering::SeqCst), 1);
    assert_eq!(j.len(), 1, "the failed run left its checkpoint");

    // Another process, the same journal, a new model client and tool objects.
    let (transfer, used_again) = mutating();
    let (model, seen) = Scripted::new(vec![text("done")]);
    let answer = run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default())
        .await
        .unwrap();
    assert_eq!(answer.text, "done");
    assert_eq!(used_again.load(Ordering::SeqCst), 0, "the call ran again");
    let told = results(&seen.lock().unwrap().clone(), 0);
    assert_eq!(
        told.len(),
        1,
        "the model was shown the earlier result: {told:?}"
    );
    // What the first process showed its caller is in the resumed run's trace.
    assert_eq!(answer.trace.len(), 1, "{:?}", answer.trace);
    assert!(
        answer.trace[0].starts_with("transfer("),
        "{:?}",
        answer.trace
    );
    // A finished run is kept, completed, with its answer (N04).
    assert_eq!(
        (j.len(), j.unfinished()),
        (1, 0),
        "a finished run is kept, completed"
    );
    let kept = j.load(KEY).await.unwrap().unwrap();
    assert!(kept.completed);
    assert_eq!(
        kept.checkpoint().unwrap().answer.map(|a| a.text),
        Some(answer.text.clone())
    );
}

/// The control: with no journal the same death loses the run, and the tool is
/// run again by the next attempt.
#[tokio::test]
async fn without_a_journal_the_same_death_repeats_the_call() {
    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer(), model_fails()]);
    let first = run_keyed(
        &ToolLoop::new(model, tools(vec![transfer])),
        KEY,
        Side::default(),
    )
    .await;
    assert!(first.is_err());
    let (transfer, used_again) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer(), text("done")]);
    run_keyed(
        &ToolLoop::new(model, tools(vec![transfer])),
        KEY,
        Side::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        used.load(Ordering::SeqCst) + used_again.load(Ordering::SeqCst),
        2
    );
}

/// A different message is a different run, not a resume.
#[tokio::test]
async fn another_message_under_the_same_key_is_refused_not_resumed() {
    let j = journal();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer(), model_fails()]);
    let _ = run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default()).await;
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![text("done")]);
    let other = looped(model, vec![transfer], &j)
        .run_drafting(
            KEY,
            "a different request",
            &CancellationToken::new(),
            None,
            Side::default(),
        )
        .await;
    assert!(
        matches!(other, Err(LoopError::PinMismatch(ref w)) if w.contains("message")),
        "{other:?}"
    );
}

// ── Pins ─────────────────────────────────────────────────────────────────────

/// A run resumed under a different model, tool set, preamble, configuration or
/// pinned version is refused, naming which.
#[tokio::test]
async fn a_run_resumed_under_something_else_is_refused() {
    let j = journal();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer(), model_fails()]);
    let first = looped(model, vec![transfer], &j)
        .with_preamble("be careful")
        .with_pin("policy", "v1");
    let _ = run_keyed(&first, KEY, Side::default()).await;

    let attempt = |change: &str| {
        let (transfer, _) = mutating();
        let (model, _) = Scripted::new(vec![text("done")]);
        let base = looped(model, vec![transfer], &j)
            .with_preamble("be careful")
            .with_pin("policy", "v1");
        match change {
            "preamble" => base.with_preamble("be reckless"),
            "pin" => base.with_pin("policy", "v2"),
            "config" => base.with_max_turns(3),
            _ => base,
        }
    };
    for (change, says) in [
        ("preamble", "preamble"),
        ("pin", "pinned version"),
        ("config", "configuration"),
    ] {
        let r = run_keyed(&attempt(change), KEY, Side::default()).await;
        assert!(
            matches!(&r, Err(LoopError::PinMismatch(w)) if w.contains(says)),
            "{change}: {r:?}"
        );
    }
    // A different tool set, and a different model.
    let (model, _) = Scripted::new(vec![text("done")]);
    let r = run_keyed(
        &looped(model, vec![], &j)
            .with_preamble("be careful")
            .with_pin("policy", "v1"),
        KEY,
        Side::default(),
    )
    .await;
    assert!(
        matches!(&r, Err(LoopError::PinMismatch(w)) if w.contains("tool set")),
        "{r:?}"
    );
    let (transfer, _) = mutating();
    let r = run_keyed(
        &ToolLoop::new(OtherModel, tools(vec![transfer]))
            .with_journal(Arc::clone(&j) as Arc<dyn StepJournal>)
            .with_preamble("be careful")
            .with_pin("policy", "v1"),
        KEY,
        Side::default(),
    )
    .await;
    assert!(
        matches!(&r, Err(LoopError::PinMismatch(w)) if w.contains("model")),
        "{r:?}"
    );
    // Unchanged, it resumes.
    assert!(run_keyed(&attempt(""), KEY, Side::default()).await.is_ok());
}

/// A model of another name, which answers at once (a run wrongly continued under
/// it ends instead of hanging).
struct OtherModel;
impl ToolModel for OtherModel {
    fn model_name(&self) -> &'static str {
        "another-model"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(async { Ok(ModelTurn::new("done", Vec::new())) })
    }
}

// ── Receipts and fail-closed ─────────────────────────────────────────────────

/// Blocks inside the call until released: a replica dying mid-call.
pub(super) struct Blocks {
    entered: Arc<tokio::sync::Notify>,
    keys: Arc<Mutex<Vec<String>>>,
    calls: Arc<AtomicUsize>,
    /// Whether its first call never returns.
    hangs_first: bool,
}

impl Tool for Blocks {
    fn name(&self) -> &'static str {
        "transfer"
    }
    fn description(&self) -> &'static str {
        "a test tool"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"service": {"type": "string"}}})
    }
    fn call<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.keys.lock().unwrap().push(invocation.idempotency_key);
        let entered = Arc::clone(&self.entered);
        let blocks = self.hangs_first && self.calls.load(Ordering::SeqCst) == 1;
        Box::pin(async move {
            if blocks {
                entered.notify_one();
                std::future::pending::<()>().await;
            }
            Ok("done".to_owned())
        })
    }
}

pub(super) fn blocking(
    keys: &Arc<Mutex<Vec<String>>>,
    blocks_first: bool,
) -> (Blocks, Arc<tokio::sync::Notify>, Arc<AtomicUsize>) {
    let entered = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let tool = Blocks {
        entered: Arc::clone(&entered),
        keys: Arc::clone(keys),
        calls: Arc::clone(&calls),
        hangs_first: blocks_first,
    };
    (tool, entered, calls)
}

fn loop_over(
    model: Scripted,
    tool: Blocks,
    j: &Arc<InMemoryStepJournal>,
    secret: bool,
) -> ToolLoop {
    let mut set = ToolSet::new();
    set.add(tool).unwrap();
    let base = ToolLoop::new(model, set).with_journal(Arc::clone(j) as Arc<dyn StepJournal>);
    if secret {
        base.with_idempotency_secret([9; 32])
    } else {
        base
    }
}

/// The receipt is written before the call runs, so a replica that dies in the
/// middle of a call leaves it as `Started`.
async fn died_mid_call(j: &Arc<InMemoryStepJournal>, keys: &Arc<Mutex<Vec<String>>>) {
    let (tool, entered, _) = blocking(keys, true);
    let (model, _) = Scripted::new(vec![one_transfer()]);
    let tool_loop = loop_over(model, tool, j, true);
    let mut run = Box::pin(run_keyed(&tool_loop, KEY, Side::default()));
    tokio::select! {
        r = &mut run => panic!("the run ended: {r:?}"),
        () = entered.notified() => {}
    }
    // The process is gone: the future is dropped inside the call.
}

#[tokio::test]
async fn a_call_that_may_have_run_is_rerun_under_the_same_key_when_the_secret_is_shared() {
    let j = journal();
    let keys = Arc::new(Mutex::new(Vec::new()));
    died_mid_call(&j, &keys).await;
    let stored = j
        .load(KEY)
        .await
        .unwrap()
        .expect("a checkpoint")
        .checkpoint()
        .unwrap();
    assert_eq!(
        stored.receipts.iter().map(|r| r.state).collect::<Vec<_>>(),
        [ReceiptState::Started]
    );
    assert!(stored.receipts[0].arguments.starts_with("sha256:"));
    assert!(
        !format!("{:?}", stored.receipts).contains("\"x\""),
        "a receipt holds the arguments"
    );

    let (tool, _, calls) = blocking(&keys, false);
    let (model, _) = Scripted::new(vec![text("done")]);
    let answer = run_keyed(&loop_over(model, tool, &j, true), KEY, Side::default())
        .await
        .unwrap();
    assert_eq!(answer.text, "done");
    // The second process ran the call (the first died inside it) under the key the first had.
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let keys = keys.lock().unwrap().clone();
    assert_eq!(keys.len(), 2);
    assert_eq!(
        keys[0], keys[1],
        "a backend could not deduplicate the retry"
    );
}

#[tokio::test]
async fn a_call_that_may_have_run_is_not_rerun_when_its_key_would_differ() {
    let j = journal();
    let keys = Arc::new(Mutex::new(Vec::new()));
    died_mid_call(&j, &keys).await;
    let (tool, _, calls) = blocking(&keys, false);
    let (model, _) = Scripted::new(vec![text("done")]);
    let r = run_keyed(&loop_over(model, tool, &j, false), KEY, Side::default()).await;
    assert!(
        matches!(&r, Err(LoopError::Indeterminate { tool, call_id }) if tool == "transfer" && call_id == "c1"),
        "{r:?}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "it ran against a key nobody can match"
    );
}

/// A checkpoint of another format version is refused, not guessed at.
#[tokio::test]
async fn a_checkpoint_of_another_version_is_refused() {
    let j = journal();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer(), model_fails()]);
    let _ = run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default()).await;
    let entry = j.load(KEY).await.unwrap().unwrap();
    j.clear(KEY, entry.revision).await.unwrap();
    let mut stored = entry.checkpoint().unwrap();
    stored.version = 99;
    stored.revision = 1;
    let mut entry = entry;
    entry.revision = 1;
    entry.body = crate::tool_loop::journal::to_text(&stored).unwrap();
    j.save(KEY, &entry).await.unwrap();
    let (transfer, used) = mutating();
    let (model, _) = Scripted::new(vec![text("done")]);
    let r = run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default()).await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("version 99")),
        "{r:?}"
    );
    assert_eq!(used.load(Ordering::SeqCst), 0);
}

/// A resumed run has used the turns it took before: a loop bounded to one turn
/// that took it does not ask the model again.
#[tokio::test]
async fn a_resumed_run_counts_the_turns_it_already_took() {
    let j = journal();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer()]);
    let first = looped(model, vec![transfer], &j).with_max_turns(1);
    let r = run_keyed(&first, KEY, Side::default()).await;
    assert!(matches!(r, Err(LoopError::TurnLimit(1))), "{r:?}");

    let (transfer, used) = mutating();
    let (model, seen) = Scripted::new(vec![text("a turn the bound does not allow")]);
    let second = looped(model, vec![transfer], &j).with_max_turns(1);
    let r = run_keyed(&second, KEY, Side::default()).await;
    assert!(matches!(r, Err(LoopError::TurnLimit(1))), "{r:?}");
    assert_eq!(
        seen.lock().unwrap().len(),
        0,
        "the model was asked past the bound"
    );
    assert_eq!(used.load(Ordering::SeqCst), 0);
}
