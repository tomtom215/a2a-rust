// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Call ids that name one call each, and a run whose bytes are bounded
//! (adversarial review N08 and N09, 2026-11-13).

use super::journal::{KEY, journal, looped, run_keyed};
use super::*;
use crate::tool_loop::journal::StepJournal;
use crate::tool_loop::steering::Side;

/// N08: a reply naming two calls with one id is refused before either runs:
/// their results could not be told apart, by the provider or by the journal.
#[tokio::test]
async fn a_reply_naming_one_id_twice_runs_nothing() {
    let j = journal();
    let (transfer, used) = TestTool::new("transfer", |_| Ok("sent".to_owned()));
    let (model, _) = Scripted::new(vec![calls(vec![
        call("c1", "transfer", r#"{"to":"a"}"#),
        call("c1", "transfer", r#"{"to":"b"}"#),
    ])]);
    let r = run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default()).await;
    assert_eq!(
        r.map(|a| a.text),
        Err(LoopError::AmbiguousCalls {
            id: "c1".to_owned()
        })
    );
    assert_eq!(
        used.load(Ordering::SeqCst),
        0,
        "a call of an ambiguous reply ran"
    );
    assert!(j.is_empty(), "an ambiguous reply was journaled");
}

/// N08: a provider may use an id again in a later turn. Each call has a
/// receipt of its own: until 2026-11-13 receipts were found by id alone, so
/// the second call updated the first's and had none.
#[tokio::test]
async fn an_id_used_again_in_a_later_turn_has_a_receipt_of_its_own() {
    let j = journal();
    let (transfer, used) = TestTool::new("transfer", |_| Ok("sent".to_owned()));
    let (model, _) = Scripted::new(vec![
        calls(vec![call("c1", "transfer", r#"{"to":"a"}"#)]),
        calls(vec![call("c1", "transfer", r#"{"to":"b"}"#)]),
        Err(ModelError::Other("the replica died".to_owned())),
    ]);
    let _ = run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default()).await;
    assert_eq!(used.load(Ordering::SeqCst), 2);
    let cp = j.load(KEY).await.unwrap().unwrap().checkpoint().unwrap();
    let receipts: Vec<(usize, &str, ReceiptState)> = cp
        .receipts
        .iter()
        .map(|r| (r.turn, r.call_id.as_str(), r.state))
        .collect();
    assert_eq!(
        receipts,
        vec![(0, "c1", ReceiptState::Ran), (1, "c1", ReceiptState::Ran)]
    );
    assert_ne!(cp.receipts[0].arguments, cp.receipts[1].arguments);
}

/// N08: an id the loop gives a call that came without one is not one the
/// provider already used in the reply.
#[tokio::test]
async fn a_generated_id_does_not_take_a_provider_id() {
    let (transfer, used) = TestTool::new("transfer", |args| Ok(format!("sent {args}")));
    let (model, seen) = Scripted::new(vec![
        calls(vec![
            call("call-0-1", "transfer", r#"{"to":"a"}"#),
            call("", "transfer", r#"{"to":"b"}"#),
        ]),
        text("done"),
    ]);
    let answer = ToolLoop::new(model, tools(vec![transfer]))
        .run("go", &tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(answer.text, "done");
    assert_eq!(used.load(Ordering::SeqCst), 2);
    let told = results(&seen.lock().unwrap().clone(), 1);
    let ids: Vec<&str> = told.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, vec!["call-0-1", "call-0-1-1"]);
}

/// N08: when the provider already used the first suffixed name too, the
/// generated id counts on to the next one (`-2`), as the rule says.
#[tokio::test]
async fn a_generated_id_counts_past_every_taken_suffix() {
    let (transfer, used) = TestTool::new("transfer", |args| Ok(format!("sent {args}")));
    let (model, seen) = Scripted::new(vec![
        calls(vec![
            call("call-0-1", "transfer", r#"{"to":"a"}"#),
            call("", "transfer", r#"{"to":"b"}"#),
            call("call-0-1-1", "transfer", r#"{"to":"c"}"#),
        ]),
        text("done"),
    ]);
    let answer = ToolLoop::new(model, tools(vec![transfer]))
        .run("go", &tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(answer.text, "done");
    assert_eq!(used.load(Ordering::SeqCst), 3);
    let told = results(&seen.lock().unwrap().clone(), 1);
    let ids: Vec<&str> = told.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, vec!["call-0-1", "call-0-1-2", "call-0-1-1"]);
}

/// N09: a tool's result past the limit is cut, and the model is told so.
#[tokio::test]
async fn a_large_result_is_cut_and_the_model_is_told() {
    let (dump, _) = TestTool::new("dump", |_| Ok("x".repeat(100)));
    let (model, seen) = Scripted::new(vec![calls(vec![call("c1", "dump", "{}")]), text("ok")]);
    let answer = ToolLoop::new(model, tools(vec![dump]))
        .with_result_limit(10)
        .run("go", &tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    let told = results(&seen.lock().unwrap().clone(), 1);
    assert_eq!(
        told[0].1,
        format!(
            "{}\n[the tool's result was 100 bytes; only the first 10 are shown here]",
            "x".repeat(10)
        )
    );
    assert!(answer.trace[0].len() < 200, "{:?}", answer.trace);
}

/// N09: a model request past the limit is not sent; the run ends saying how
/// large it was.
#[tokio::test]
async fn a_request_past_the_limit_is_not_sent() {
    let (dump, _) = TestTool::new("dump", |_| Ok("y".repeat(5_000)));
    let (model, seen) = Scripted::new(vec![
        calls(vec![call("c1", "dump", "{}")]),
        text("never asked"),
    ]);
    let r = ToolLoop::new(model, tools(vec![dump]))
        .with_request_limit(4_000)
        .run("go", &tokio_util::sync::CancellationToken::new())
        .await;
    assert!(
        matches!(r, Err(LoopError::TooLarge { bytes, limit: 4_000 }) if bytes > 5_000),
        "{r:?}"
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "the oversized request was sent"
    );
}

/// N09: a checkpoint past the limit is not written, and the call it was to
/// record does not run.
#[tokio::test]
async fn a_checkpoint_past_the_limit_stops_the_run_before_the_call() {
    let j = journal();
    let (transfer, used) = TestTool::new("transfer", |_| Ok("sent".to_owned()));
    let (model, _) = Scripted::new(vec![calls(vec![call(
        "c1",
        "transfer",
        &format!(r#"{{"memo":"{}"}}"#, "z".repeat(2_000)),
    )])]);
    let r = run_keyed(
        &ToolLoop::new(model, tools(vec![transfer]))
            .with_journal(Arc::clone(&j) as Arc<dyn StepJournal>)
            .with_checkpoint_limit(1_000),
        KEY,
        Side::default(),
    )
    .await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("checkpoint limit")),
        "{r:?}"
    );
    assert_eq!(used.load(Ordering::SeqCst), 0);
    assert!(j.is_empty());
}
