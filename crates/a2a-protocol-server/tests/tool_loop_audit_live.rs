// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An approved tool call through the real handler, with a **real open-source
//! model** served by `llama-server` (adversarial review of 2026-11-13, N01, N02
//! and N07).
//!
//! `an_approved_call_is_audited_naming_no_one_and_its_run_erases`: the approval
//! gate and a review grant, a policy wrapped in `AuditedPolicy` under audit
//! privacy, and a sealed SQLite journal. The model's transfer waits for Bob,
//! runs once when he approves, and is recorded as `approval_required` and then
//! `approved`, the approval naming Bob only sealed, in an audit file whose
//! bytes name neither person. The run's journal entry is listed for Alice and
//! erased for her.
//!
//! Writing this test found a hang on single-worker runtimes (the register's
//! D1): the blocking send's collector stopped polling the executor it ran while
//! it folded an event, and the executor held the audit chain lock the fold
//! needed. `A2A_LIVE_TRACE=1` prints every model turn and the handler's trace.
//!
//! ```bash
//! A2A_LIVE_MODEL_URL=http://127.0.0.1:11434/v1 A2A_LIVE_MODEL=qwen3:1.7b \
//!   cargo test -p a2a-protocol-server --features "tool-loop sqlite audit encryption" \
//!   --test tool_loop_audit_live -- --ignored --test-threads=1 --nocapture
//! ```

#![cfg(all(
    feature = "tool-loop",
    feature = "sqlite",
    feature = "audit",
    feature = "encryption"
))]

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_server::approval::{ApprovalGate, REVIEW_FOR_HEADER};
use a2a_protocol_server::audit::record::kind;
use a2a_protocol_server::audit::{
    AuditLog, AuditPrivacy, AuditStore, HMAC_KEY_LEN, SEALED_VALUE_PREFIX, SqliteAuditStore,
};
use a2a_protocol_server::auth::caller_id;
use a2a_protocol_server::store::encrypted::{AES_256_KEY_LEN, Aes256GcmCipher};
use a2a_protocol_server::tool_loop::{
    AuditedPolicy, ChatRequest, ModelError, ModelTurn, OpenAiChatModel, PolicyDecision,
    PolicyRequest, SqliteStepJournal, StepJournal, Tool, ToolError, ToolInvocation, ToolLoop,
    ToolLoopExecutor, ToolModel, ToolPolicy, ToolSet,
};
use a2a_protocol_server::{
    BearerTokenAuthInterceptor, RequestHandler, RequestHandlerBuilder, SendMessageResult,
};
use a2a_protocol_types::approval::{ApprovalDecision, ApprovalRequest};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{Task, TaskState};
use serde_json::{Value, json};

#[path = "support/live_model.rs"]
mod live_model;
use live_model::{DbFile, cipher, model};

/// Prints each turn asked of the model and what came back, when
/// `A2A_LIVE_TRACE` is set: what a run did is otherwise not visible.
struct Logged(OpenAiChatModel);

impl ToolModel for Logged {
    fn model_name(&self) -> &str {
        self.0.model_name()
    }
    fn provider_name(&self) -> &str {
        self.0.provider_name()
    }
    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(async move {
            let last = request
                .messages
                .last()
                .map(|m| format!("{m:?}"))
                .unwrap_or_default();
            let reply = self.0.complete(request).await;
            if std::env::var_os("A2A_LIVE_TRACE").is_some() {
                let last: String = last.chars().take(240).collect();
                eprintln!(
                    "[model] {} messages, last {last}\n[model] -> {reply:?}",
                    request.messages.len()
                );
            }
            reply
        })
    }
}

const ALICE: &str = "bearer:alice-live-n02";

struct Transfer {
    ran: Arc<AtomicUsize>,
}

impl Tool for Transfer {
    fn name(&self) -> &'static str {
        "transfer_money"
    }
    fn description(&self) -> &'static str {
        "Transfers money out of an account. Changes the account."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"account": {"type": "string"}, "amount": {"type": "number"}}, "required": ["account"]})
    }
    fn call<'a>(
        &'a self,
        _invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        self.ran.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok("transfer done".to_owned()) })
    }
}

struct NeedsApproval;

impl ToolPolicy for NeedsApproval {
    fn authorize<'a>(
        &'a self,
        request: &'a PolicyRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = PolicyDecision> + Send + 'a>> {
        let decision = if request.tool == "transfer_money" {
            PolicyDecision::RequireApproval(format!("transfer_money {}", request.arguments))
        } else {
            PolicyDecision::Allow
        };
        Box::pin(async move { decision })
    }
}

fn as_user(token: &str, requester: Option<&str>) -> HashMap<String, String> {
    let mut h = HashMap::from([("authorization".to_owned(), format!("Bearer {token}"))]);
    if let Some(r) = requester {
        h.insert(REVIEW_FOR_HEADER.to_owned(), r.to_owned());
    }
    h
}

/// A send, bounded: a turn that does not come back in five minutes (the
/// model's own timeout) fails the test rather than holding it until the
/// executor's hour.
async fn send(h: &RequestHandler, who: HashMap<String, String>, m: Message) -> Task {
    let sent = tokio::time::timeout(
        Duration::from_secs(300),
        h.on_send_message(MessageSendParams::new(m), false, Some(&who)),
    )
    .await
    .expect("the send did not return within five minutes");
    match sent.expect("the send is admitted") {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t,
        other => panic!("a task: {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a llama-server (A2A_LIVE_MODEL_URL)"]
async fn an_approved_call_is_audited_naming_no_one_and_its_run_erases() {
    if std::env::var_os("A2A_LIVE_TRACE").is_some() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing_subscriber::filter::LevelFilter::DEBUG)
            .with_test_writer()
            .try_init();
    }
    let (audit_db, journal_db) = (DbFile::new("audit"), DbFile::new("journal"));
    let store: Arc<dyn AuditStore> =
        Arc::new(SqliteAuditStore::new(&audit_db.url()).await.unwrap());
    let kek = Arc::new(Aes256GcmCipher::new("kek-1", [5; AES_256_KEY_LEN]).unwrap());
    let log = Arc::new(
        AuditLog::new(store)
            .with_privacy(AuditPrivacy::new(kek, "h1", [6; HMAC_KEY_LEN]).unwrap())
            .unwrap(),
    );
    let journal = Arc::new(SqliteStepJournal::new(&journal_db.url()).await.unwrap());
    let ran = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolSet::new();
    tools
        .add(Transfer {
            ran: Arc::clone(&ran),
        })
        .unwrap();
    let tool_loop = ToolLoop::new(Logged(model()), tools)
        .with_preamble(
            "You are a banking assistant. Use the tools when the user asks for an action.",
        )
        .with_max_turns(4)
        .with_policy(AuditedPolicy::new(Arc::clone(&log)).wrapping(NeedsApproval))
        .with_journal(Arc::clone(&journal) as Arc<dyn StepJournal>)
        .with_journal_cipher(cipher(9));
    let handler = RequestHandlerBuilder::new(ToolLoopExecutor::new(tool_loop))
        .without_agent_card()
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
            ("t-alice", "alice-live-n02"),
            ("t-bob", "bob-live-n02"),
        ]))
        .with_approval_gate(
            ApprovalGate::new()
                .with_approvers([caller_id::bearer("bob-live-n02")])
                .with_review_grants(),
        )
        .with_audit(Arc::clone(&log))
        .build()
        .unwrap();

    let ask = Message::new(
        MessageId::new("m-ask"),
        MessageRole::User,
        vec![Part::text(
            "Transfer 500 dollars out of account 4417 right now using transfer_money.",
        )],
    );
    let task = send(&handler, as_user("t-alice", None), ask).await;
    assert_eq!(
        task.status.state,
        TaskState::InputRequired,
        "the model made no transfer call, so nothing was tested; rerun or change the prompt: {task:?}"
    );
    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "it ran before it was approved"
    );
    let req = ApprovalRequest::read(task.status.message.as_ref().unwrap())
        .unwrap()
        .expect("the approval request");
    let mut yes = Message::user_text("a-1", "approved");
    yes.task_id = Some(task.id.clone());
    yes.context_id = Some(task.context_id.clone());
    ApprovalDecision::approve(&req).attach(&mut yes);
    let done = send(&handler, as_user("t-bob", Some(ALICE)), yes).await;
    println!(
        "after approval: {:?}; tool ran {} time(s)",
        done.status.state,
        ran.load(Ordering::SeqCst)
    );
    assert_eq!(done.status.state, TaskState::Completed, "{done:?}");
    assert_eq!(ran.load(Ordering::SeqCst), 1, "the approved call ran once");

    // The tool's records: asked for approval, then approved, the approver sealed.
    let actions: Vec<_> = log
        .export("")
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == kind::TOOL_ACTION)
        .collect();
    let decisions: Vec<&str> = actions
        .iter()
        .filter_map(|r| r.detail.get("decision").and_then(Value::as_str))
        .collect();
    println!("tool.action decisions: {decisions:?}");
    assert_eq!(
        decisions.first(),
        Some(&"approval_required"),
        "{decisions:?}"
    );
    assert_eq!(decisions.last(), Some(&"approved"), "{decisions:?}");
    assert_eq!(
        decisions.iter().filter(|d| **d == "approved").count(),
        1,
        "{decisions:?}"
    );
    let approved = actions.last().unwrap();
    let by = approved.detail["approvedBy"][0].as_str().unwrap();
    assert!(
        by.starts_with(SEALED_VALUE_PREFIX),
        "approvedBy in the clear: {by}"
    );
    for name in ["alice-live-n02", "bob-live-n02"] {
        assert!(!audit_db.holds(name), "the audit file names {name}");
    }

    // The run is Alice's, finished, sealed, and erased for her.
    let runs = journal.runs_of(ALICE, None).await.unwrap();
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert!(runs[0].completed, "{runs:?}");
    assert!(
        !journal_db.holds("account 4417"),
        "the journal file holds the prompt"
    );
    assert_eq!(journal.erase(ALICE, None).await.unwrap(), 1);
    assert!(journal.runs_of(ALICE, None).await.unwrap().is_empty());
    assert!(journal.load(&runs[0].key).await.unwrap().is_none());
}
