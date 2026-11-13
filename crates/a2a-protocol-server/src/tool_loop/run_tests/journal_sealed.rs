// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A journal whose checkpoints are sealed (adversarial review N07,
//! 2026-11-13): the conversation, a tool's arguments and its result are not
//! in what the store holds, and a run still resumes through the cipher.
//!
//! Until then every checkpoint was JSON in a TEXT column, whatever the task
//! store's or the audit log's encryption: the journal was a second, plaintext
//! copy of each run's content.

use super::journal::{KEY, journal, model_fails};
use super::*;
use crate::store::encrypted::{AES_256_KEY_LEN, Aes256GcmCipher, ContentCipher};
use crate::tool_loop::journal::{SEALED_CHECKPOINT_PREFIX, StepJournal};
use crate::tool_loop::steering::Side;

const CANARY_PROMPT: &str = "patient 4471 has condition X";
const CANARY_ARGUMENT: &str = "iban-DE89370400440532013000";
const CANARY_RESULT: &str = "balance 9182.11";

fn cipher() -> Arc<dyn ContentCipher> {
    Arc::new(Aes256GcmCipher::new("journal-1", [3; AES_256_KEY_LEN]).unwrap())
}

fn sealed_loop(
    model: Scripted,
    j: &Arc<crate::tool_loop::InMemoryStepJournal>,
    calls_made: &Arc<AtomicUsize>,
) -> ToolLoop {
    let calls_made = Arc::clone(calls_made);
    let (tool, _) = TestTool::new("lookup", move |_| {
        calls_made.fetch_add(1, Ordering::SeqCst);
        Ok(CANARY_RESULT.to_owned())
    });
    ToolLoop::new(model, tools(vec![tool]))
        .with_journal(Arc::clone(j) as Arc<dyn StepJournal>)
        .with_journal_cipher(cipher())
        .with_idempotency_secret([7; 32])
}

async fn run(tool_loop: &ToolLoop) -> Result<Answer, LoopError> {
    tool_loop
        .run_drafting(
            KEY,
            CANARY_PROMPT,
            &tokio_util::sync::CancellationToken::new(),
            None,
            Side::default(),
        )
        .await
}

#[tokio::test]
async fn a_sealed_journal_holds_no_content_and_still_resumes() {
    let j = journal();
    let made = Arc::new(AtomicUsize::new(0));
    let (model, _) = Scripted::new(vec![
        calls(vec![call(
            "c1",
            "lookup",
            &format!(r#"{{"account":"{CANARY_ARGUMENT}"}}"#),
        )]),
        model_fails(),
    ]);
    let r = run(&sealed_loop(model, &j, &made)).await;
    assert!(matches!(r, Err(LoopError::Model(_))), "{r:?}");
    assert_eq!(made.load(Ordering::SeqCst), 1);

    let stored = j.load(KEY).await.unwrap().unwrap();
    assert!(
        stored.body.starts_with(SEALED_CHECKPOINT_PREFIX),
        "{}",
        stored.body
    );
    for canary in [CANARY_PROMPT, CANARY_ARGUMENT, CANARY_RESULT, "lookup"] {
        assert!(
            !stored.body.contains(canary),
            "{canary} is in the stored body"
        );
    }
    assert!(stored.checkpoint().is_err(), "read as JSON without the key");

    // Resumes through the cipher: the call is not made again.
    let (model, seen) = Scripted::new(vec![text("found it")]);
    let answer = run(&sealed_loop(model, &j, &made)).await.unwrap();
    assert_eq!(answer.text, "found it");
    assert_eq!(made.load(Ordering::SeqCst), 1, "the call was made again");
    // The model is given the restored conversation: the call's one result.
    let told = results(&seen.lock().unwrap().clone(), 0);
    assert_eq!(told, vec![("c1".to_owned(), CANARY_RESULT.to_owned())]);
    let finished = j.load(KEY).await.unwrap().unwrap();
    assert!(finished.completed);
    assert!(
        !finished.body.contains("found it"),
        "the answer is in the clear"
    );
}

#[tokio::test]
async fn a_sealed_body_does_not_open_elsewhere_or_without_the_cipher() {
    let j = journal();
    let made = Arc::new(AtomicUsize::new(0));
    let (model, _) = Scripted::new(vec![calls(vec![call("c1", "lookup", "{}")]), model_fails()]);
    let _ = run(&sealed_loop(model, &j, &made)).await;
    let stored = j.load(KEY).await.unwrap().unwrap();

    // Moved to another run's key: refused.
    let other = journal();
    let mut moved = stored.clone();
    moved.revision = 1;
    other.save(KEY, &moved).await.unwrap();
    let (model, _) = Scripted::new(vec![text("x")]);
    let r = run(&sealed_loop(model, &other, &made)).await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("did not open")),
        "a body sealed for another revision opened: {r:?}"
    );

    // Without the cipher: refused, not read as a fresh run.
    let (tool, _) = TestTool::new("lookup", |_| Ok(String::new()));
    let (model, seen) = Scripted::new(vec![text("x")]);
    let plain = ToolLoop::new(model, tools(vec![tool]))
        .with_journal(Arc::clone(&j) as Arc<dyn StepJournal>)
        .with_idempotency_secret([7; 32]);
    let r = run(&plain).await;
    assert!(
        matches!(&r, Err(LoopError::Journal(w)) if w.contains("no journal cipher")),
        "{r:?}"
    );
    assert_eq!(seen.lock().unwrap().len(), 0);
}

/// A journal written in the clear before the cipher was set is read, and
/// sealed from its next write.
#[tokio::test]
async fn a_clear_checkpoint_is_read_and_sealed_from_its_next_write() {
    let j = journal();
    let made = Arc::new(AtomicUsize::new(0));
    let made_clear = Arc::clone(&made);
    let (tool, _) = TestTool::new("lookup", move |_| {
        made_clear.fetch_add(1, Ordering::SeqCst);
        Ok(CANARY_RESULT.to_owned())
    });
    let (model, _) = Scripted::new(vec![calls(vec![call("c1", "lookup", "{}")]), model_fails()]);
    let clear = ToolLoop::new(model, tools(vec![tool]))
        .with_journal(Arc::clone(&j) as Arc<dyn StepJournal>)
        .with_idempotency_secret([7; 32]);
    let _ = run(&clear).await;
    assert!(
        j.load(KEY)
            .await
            .unwrap()
            .unwrap()
            .body
            .contains(CANARY_RESULT)
    );

    let (model, _) = Scripted::new(vec![text("done")]);
    run(&sealed_loop(model, &j, &made)).await.unwrap();
    let after = j.load(KEY).await.unwrap().unwrap();
    assert!(after.body.starts_with(SEALED_CHECKPOINT_PREFIX));
    assert!(!after.body.contains(CANARY_RESULT));
    assert_eq!(made.load(Ordering::SeqCst), 1);
}
