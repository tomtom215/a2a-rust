// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The fixes for the adversarial review of 2026-11-13, with a **real
//! open-source model** served by `llama-server`: what the unit tests prove with
//! a scripted model, here with one that chooses its own calls and words.
//!
//! * `a_finished_run_is_sealed_and_replayed_without_the_model_or_the_tool` (N04,
//!   N07): a run that answers is journaled as finished, sealed, in a SQLite
//!   file whose bytes hold neither the prompt nor the tool's result; the same
//!   key run again by a new loop over the same file returns the same answer
//!   without asking the model or calling the tool; a loop with another key
//!   cannot open it.
//! * `a_large_tool_result_is_cut_before_the_model_sees_it` (N09): a tool returns
//!   over 200,000 bytes; the model is sent at most the limit and the note saying
//!   what was cut, and still answers.
//!
//! The approval and audit path is in `tool_loop_audit_live.rs`.
//!
//! ```bash
//! llama-server -m Qwen3-1.7B-Q4_K_M.gguf --port 11434 --alias qwen3:1.7b --jinja -c 4096 -np 1
//! A2A_LIVE_MODEL_URL=http://127.0.0.1:11434/v1 A2A_LIVE_MODEL=qwen3:1.7b \
//!   cargo test -p a2a-protocol-server --features "tool-loop sqlite audit encryption" \
//!   --test tool_loop_review_live -- --ignored --test-threads=1 --nocapture
//! ```

#![cfg(all(
    feature = "tool-loop",
    feature = "sqlite",
    feature = "audit",
    feature = "encryption"
))]

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use a2a_protocol_server::tool_loop::{
    ChatMessage, ChatRequest, ModelError, ModelTurn, OpenAiChatModel, SEALED_CHECKPOINT_PREFIX,
    SqliteStepJournal, StepJournal, Tool, ToolError, ToolInvocation, ToolLoop, ToolModel, ToolSet,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[path = "support/live_model.rs"]
mod live_model;
use live_model::{DbFile, cipher, model};

/// Counts the turns asked of the real model, and keeps the longest tool
/// result each request carried.
struct Counting {
    inner: OpenAiChatModel,
    asked: Arc<AtomicUsize>,
    longest_result: Arc<Mutex<String>>,
}

impl Counting {
    fn new(asked: &Arc<AtomicUsize>) -> Self {
        Self {
            inner: model(),
            asked: Arc::clone(asked),
            longest_result: Arc::default(),
        }
    }
}

impl ToolModel for Counting {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }
    fn provider_name(&self) -> &str {
        self.inner.provider_name()
    }
    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        for m in request.messages {
            if let ChatMessage::ToolResult { content, .. } = m {
                let mut longest = self.longest_result.lock().unwrap();
                if content.len() > longest.len() {
                    longest.clone_from(content);
                }
            }
        }
        self.inner.complete(request)
    }
}

/// `get_balance`: counts its calls and returns `reply`.
struct Balance {
    calls: Arc<AtomicUsize>,
    reply: String,
}

impl Tool for Balance {
    fn name(&self) -> &'static str {
        "get_balance"
    }
    fn description(&self) -> &'static str {
        "Looks up the balance of an account."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"account": {"type": "string"}}, "required": ["account"]})
    }
    fn call<'a>(
        &'a self,
        _invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let reply = self.reply.clone();
        Box::pin(async move { Ok(reply) })
    }
}

fn balance_loop(model: Counting, calls: &Arc<AtomicUsize>, reply: &str) -> ToolLoop {
    let mut tools = ToolSet::new();
    tools
        .add(Balance {
            calls: Arc::clone(calls),
            reply: reply.to_owned(),
        })
        .unwrap();
    ToolLoop::new(model, tools)
        .with_preamble("You are a banking assistant. Use the tools to look things up.")
        .with_max_turns(4)
}

const PROMPT: &str =
    "What is the balance of account 4417? Use get_balance, then answer in one sentence.";
const BALANCE: &str = "balance: 1250 EUR (account holder Mallory Quist)";

#[tokio::test]
#[ignore = "needs a llama-server (A2A_LIVE_MODEL_URL)"]
async fn a_finished_run_is_sealed_and_replayed_without_the_model_or_the_tool() {
    let db = DbFile::new("sealed-journal");
    let calls = Arc::new(AtomicUsize::new(0));
    let cancel = CancellationToken::new();

    // Process one: the real model calls the tool and answers.
    let asked_first = Arc::new(AtomicUsize::new(0));
    let journal: Arc<dyn StepJournal> = Arc::new(SqliteStepJournal::new(&db.url()).await.unwrap());
    let first = balance_loop(Counting::new(&asked_first), &calls, BALANCE)
        .with_journal(Arc::clone(&journal))
        .with_journal_cipher(cipher(9));
    let answer = first
        .run_scoped("run-sealed", PROMPT, &cancel, None)
        .await
        .expect("the run answers");
    println!(
        "process one: asked {} time(s), tool called {} time(s); answer {:?}",
        asked_first.load(Ordering::SeqCst),
        calls.load(Ordering::SeqCst),
        answer.text
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the model did not call the tool once, so nothing was tested; rerun or change the prompt"
    );

    // Journaled as finished, and sealed: the file holds neither the prompt nor
    // the tool's result nor the answer.
    let entry = journal.load("run-sealed").await.unwrap().expect("an entry");
    assert!(entry.completed, "a finished run is journaled as finished");
    assert!(
        entry.body.starts_with(SEALED_CHECKPOINT_PREFIX),
        "the checkpoint is sealed"
    );
    for clear in ["account 4417", "Mallory Quist", answer.text.trim()] {
        assert!(
            !db.holds(clear),
            "the journal file holds {clear:?} in the clear"
        );
    }

    // Process two: a new loop over the same file answers from the journal.
    let asked_second = Arc::new(AtomicUsize::new(0));
    let journal: Arc<dyn StepJournal> = Arc::new(SqliteStepJournal::new(&db.url()).await.unwrap());
    let second = balance_loop(Counting::new(&asked_second), &calls, BALANCE)
        .with_journal(Arc::clone(&journal))
        .with_journal_cipher(cipher(9));
    let again = second
        .run_scoped("run-sealed", PROMPT, &cancel, None)
        .await
        .expect("the replay answers");
    assert_eq!(again.text, answer.text, "the replay is the same answer");
    assert_eq!(
        asked_second.load(Ordering::SeqCst),
        0,
        "the replay asked the model"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the replay called the tool"
    );

    // A loop holding another key cannot open the run.
    let asked_third = Arc::new(AtomicUsize::new(0));
    let third = balance_loop(Counting::new(&asked_third), &calls, BALANCE)
        .with_journal(journal)
        .with_journal_cipher(cipher(10));
    let refused = third.run_scoped("run-sealed", PROMPT, &cancel, None).await;
    println!("another key: {refused:?}");
    assert!(
        refused.is_err(),
        "a run sealed under one key opened under another"
    );
    assert_eq!(asked_third.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// ── Byte limits ──────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "needs a llama-server (A2A_LIVE_MODEL_URL)"]
async fn a_large_tool_result_is_cut_before_the_model_sees_it() {
    const LIMIT: usize = 2048;
    let huge = format!(
        "balance: 1250 EUR\n{}",
        "ledger line 0000000000\n".repeat(9_000)
    );
    assert!(huge.len() > 200_000);
    let (asked, calls) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let model = Counting::new(&asked);
    let longest = Arc::clone(&model.longest_result);
    let run = balance_loop(model, &calls, &huge).with_result_limit(LIMIT);
    let answer = run
        .run(PROMPT, &CancellationToken::new())
        .await
        .expect("the run answers with the cut result");
    let shown = longest.lock().unwrap().clone();
    println!(
        "tool called {} time(s); the model was shown {} of {} bytes; answer {:?}",
        calls.load(Ordering::SeqCst),
        shown.len(),
        huge.len(),
        answer.text
    );
    assert!(
        calls.load(Ordering::SeqCst) >= 1,
        "the model made no call, so nothing was tested; rerun or change the prompt"
    );
    let note = format!(
        "\n[the tool's result was {} bytes; only the first {LIMIT} are shown here]",
        huge.len()
    );
    assert_eq!(shown.len(), LIMIT + note.len(), "{shown:?}");
    assert!(
        shown.ends_with(&note),
        "the model was not told what was cut"
    );
    assert!(!answer.text.trim().is_empty());
}
