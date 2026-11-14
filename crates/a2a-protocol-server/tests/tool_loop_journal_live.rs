// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The step journal and an approval bound to a call, with a **real open-source
//! model** served by `llama-server` (adversarial review S1 and S7, 2026-11-12).
//!
//! * `a_real_run_killed_inside_a_call_resumes_without_asking_for_what_it_has`: the
//!   model chooses a call; the process "dies" inside it (the future is dropped
//!   while the tool is running); a new loop over the *same SQLite file* resumes the
//!   run under the same key. The tool's second invocation carries the first one's
//!   idempotency key, and the model is asked once, not twice.
//! * `a_real_models_call_waits_for_an_approval_of_that_exact_call`: through the real
//!   handler, with the approval gate and a review grant: the model chooses
//!   `transfer_money`; the task parks `input-required` with a digest that equals
//!   the digest of the call the journal holds; an approver releases it and the tool
//!   runs once with the arguments the model chose; a refusal runs it never.
//!
//! ```bash
//! llama-server -m Qwen3-1.7B-Q4_K_M.gguf --port 11434 --alias qwen3:1.7b --jinja -c 4096 -np 1
//! A2A_LIVE_MODEL_URL=http://127.0.0.1:11434/v1 A2A_LIVE_MODEL=qwen3:1.7b \
//!   cargo test -p a2a-protocol-server --features "tool-loop sqlite" \
//!   --test tool_loop_journal_live -- --ignored --test-threads=1 --nocapture
//! ```

#![cfg(all(feature = "tool-loop", feature = "sqlite"))]

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_server::approval::{ApprovalGate, REVIEW_FOR_HEADER};
use a2a_protocol_server::auth::caller_id;
use a2a_protocol_server::tool_loop::{
    ChatRequest, InMemoryStepJournal, ModelError, ModelTurn, OpenAiChatModel, PolicyDecision,
    PolicyRequest, SqliteStepJournal, StepJournal, Tool, ToolError, ToolInvocation, ToolLoop,
    ToolLoopExecutor, ToolModel, ToolPolicy, ToolSet, call_digest,
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
use tokio_util::sync::CancellationToken;

fn model() -> OpenAiChatModel {
    let base = std::env::var("A2A_LIVE_MODEL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".to_owned());
    let name = std::env::var("A2A_LIVE_MODEL").unwrap_or_else(|_| "qwen3:1.7b".to_owned());
    let mut extra = serde_json::Map::new();
    extra.insert(
        "chat_template_kwargs".to_owned(),
        json!({"enable_thinking": false}),
    );
    extra.insert("max_tokens".to_owned(), json!(200));
    extra.insert("temperature".to_owned(), json!(0));
    extra.insert("seed".to_owned(), json!(7));
    OpenAiChatModel::new(&base, name)
        .expect("a model client")
        .with_timeout(Duration::from_secs(300))
        .with_extra_body(extra)
}

/// Counts the turns asked of the real model.
struct Counting {
    inner: OpenAiChatModel,
    asked: Arc<AtomicUsize>,
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
        self.inner.complete(request)
    }
}

/// `get_balance`, which hangs inside its first call when told to, and records
/// every invocation's idempotency key.
struct Balance {
    hang_first: bool,
    entered: Arc<tokio::sync::Notify>,
    keys: Arc<Mutex<Vec<String>>>,
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
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let first = {
            let mut keys = self.keys.lock().unwrap();
            keys.push(invocation.idempotency_key);
            keys.len() == 1
        };
        let hang = self.hang_first && first;
        let entered = Arc::clone(&self.entered);
        Box::pin(async move {
            if hang {
                entered.notify_one();
                std::future::pending::<()>().await;
            }
            Ok("balance: 1250 EUR".to_owned())
        })
    }
}

fn balance_loop(
    asked: &Arc<AtomicUsize>,
    journal: Arc<dyn StepJournal>,
    hang_first: bool,
    entered: &Arc<tokio::sync::Notify>,
    keys: &Arc<Mutex<Vec<String>>>,
) -> ToolLoop {
    let mut tools = ToolSet::new();
    tools
        .add(Balance {
            hang_first,
            entered: Arc::clone(entered),
            keys: Arc::clone(keys),
        })
        .unwrap();
    ToolLoop::new(
        Counting {
            inner: model(),
            asked: Arc::clone(asked),
        },
        tools,
    )
    .with_preamble("You are a banking assistant. Use the tools to look things up.")
    .with_max_turns(4)
    .with_journal(journal)
    .with_idempotency_secret([5; 32])
}

const PROMPT: &str =
    "What is the balance of account 4417? Use get_balance, then answer in one sentence.";

#[tokio::test]
#[ignore = "needs a llama-server (A2A_LIVE_MODEL_URL)"]
async fn a_real_run_killed_inside_a_call_resumes_without_asking_for_what_it_has() {
    let path = std::env::temp_dir().join(format!("a2a-live-journal-{}.db", uuid::Uuid::new_v4()));
    let url = format!("sqlite://{}", path.display());
    let keys = Arc::new(Mutex::new(Vec::new()));
    let (entered, asked_first) = (
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(AtomicUsize::new(0)),
    );

    // Process one: the real model chooses a call, and the process dies inside it.
    let journal: Arc<dyn StepJournal> = Arc::new(SqliteStepJournal::new(&url).await.unwrap());
    let first = balance_loop(&asked_first, journal, true, &entered, &keys);
    let cancel = CancellationToken::new();
    let mut run = Box::pin(first.run_scoped("run-1", PROMPT, &cancel, None));
    let died = tokio::time::timeout(Duration::from_secs(240), async {
        tokio::select! {
            r = &mut run => Err(r),
            () = entered.notified() => Ok(()),
        }
    })
    .await;
    match died {
        Ok(Ok(())) => {}
        other => panic!(
            "the model made no call, so nothing was tested; rerun or change the prompt: {other:?}"
        ),
    }
    drop(run);
    println!(
        "process one: the model was asked {} time(s) and died inside the call",
        asked_first.load(Ordering::SeqCst)
    );

    // Process two: a new journal handle on the same file, a new loop and new tool.
    let asked_second = Arc::new(AtomicUsize::new(0));
    let journal: Arc<dyn StepJournal> = Arc::new(SqliteStepJournal::new(&url).await.unwrap());
    let second = balance_loop(&asked_second, journal, false, &entered, &keys);
    let answer = second
        .run_scoped("run-1", PROMPT, &cancel, None)
        .await
        .expect("the resumed run answers");
    println!(
        "process two: the model was asked {} time(s); answer {:?}; trace {:?}",
        asked_second.load(Ordering::SeqCst),
        answer.text,
        answer.trace
    );
    let keys = keys.lock().unwrap().clone();
    assert_eq!(
        keys.len(),
        2,
        "the call was made in process one and again in two: {keys:?}"
    );
    assert_eq!(
        keys[0], keys[1],
        "a backend could not tell the retry from a new request"
    );
    assert_eq!(
        asked_second.load(Ordering::SeqCst),
        1,
        "the resumed run asked the model again for the turn it already had"
    );
    assert!(!answer.text.trim().is_empty());
    let _ = std::fs::remove_file(&path);
}

// ── Approval ─────────────────────────────────────────────────────────────────

struct Transfer {
    ran: Arc<AtomicUsize>,
    saw: Arc<Mutex<Vec<Value>>>,
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
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        self.ran.fetch_add(1, Ordering::SeqCst);
        self.saw.lock().unwrap().push(invocation.arguments);
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

struct Fixture {
    handler: RequestHandler,
    journal: Arc<InMemoryStepJournal>,
    ran: Arc<AtomicUsize>,
    saw: Arc<Mutex<Vec<Value>>>,
}

fn fixture() -> Fixture {
    let (ran, saw) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(Mutex::new(Vec::new())),
    );
    let journal = Arc::new(InMemoryStepJournal::new());
    let mut tools = ToolSet::new();
    tools
        .add(Transfer {
            ran: Arc::clone(&ran),
            saw: Arc::clone(&saw),
        })
        .unwrap();
    let tool_loop = ToolLoop::new(model(), tools)
        .with_preamble(
            "You are a banking assistant. Use the tools when the user asks for an action.",
        )
        .with_max_turns(4)
        .with_policy(NeedsApproval)
        .with_journal(Arc::clone(&journal) as Arc<dyn StepJournal>);
    let handler = RequestHandlerBuilder::new(ToolLoopExecutor::new(tool_loop))
        .without_agent_card()
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
            ("t-alice", "alice"),
            ("t-bob", "bob"),
        ]))
        .with_approval_gate(
            ApprovalGate::new()
                .with_approvers([caller_id::bearer("bob")])
                .with_review_grants(),
        )
        .build()
        .unwrap();
    Fixture {
        handler,
        journal,
        ran,
        saw,
    }
}

fn as_user(token: &str, requester: Option<&str>) -> HashMap<String, String> {
    let mut h = HashMap::from([("authorization".to_owned(), format!("Bearer {token}"))]);
    if let Some(r) = requester {
        h.insert(REVIEW_FOR_HEADER.to_owned(), r.to_owned());
    }
    h
}

async fn send(h: &RequestHandler, who: HashMap<String, String>, m: Message) -> Task {
    match h
        .on_send_message(MessageSendParams::new(m), false, Some(&who))
        .await
        .expect("the send is admitted")
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t,
        other => panic!("a task: {other:?}"),
    }
}

async fn parked(f: &Fixture) -> (Task, ApprovalRequest) {
    let ask = Message::new(
        MessageId::new("m-ask"),
        MessageRole::User,
        vec![Part::text(
            "Transfer 500 dollars out of account 4417 right now using transfer_money.",
        )],
    );
    let task = send(&f.handler, as_user("t-alice", None), ask).await;
    assert_eq!(
        task.status.state,
        TaskState::InputRequired,
        "the model made no transfer call, so nothing was tested; rerun or change the prompt: {task:?}"
    );
    let req = ApprovalRequest::read(task.status.message.as_ref().unwrap())
        .unwrap()
        .expect("the approval request");
    (task, req)
}

fn decision(task: &Task, req: &ApprovalRequest, approve: bool) -> Message {
    let d = if approve {
        ApprovalDecision::approve(req)
    } else {
        ApprovalDecision::deny(req)
    };
    let mut m = Message::user_text("a-1", "decided");
    m.task_id = Some(task.id.clone());
    m.context_id = Some(task.context_id.clone());
    d.attach(&mut m);
    m
}

#[tokio::test]
#[ignore = "needs a llama-server (A2A_LIVE_MODEL_URL)"]
async fn a_real_models_call_waits_for_an_approval_of_that_exact_call() {
    let f = fixture();
    let (task, req) = parked(&f).await;
    assert_eq!(
        f.ran.load(Ordering::SeqCst),
        0,
        "it ran before it was approved"
    );

    // The digest the approver is shown is the digest of the call the model made,
    // as the journal holds it.
    let held = f
        .journal
        .load(&req.request_id)
        .await
        .unwrap()
        .expect("a parked run")
        .checkpoint()
        .expect("a JSON checkpoint");
    let pending = held.pending_approval.expect("a pending call");
    let arguments: Value = serde_json::from_str(&pending.call.arguments).unwrap();
    println!("the model chose: transfer_money {arguments}");
    assert_eq!(
        req.digest,
        call_digest(
            "transfer_money",
            &arguments,
            None,
            Some("bearer:alice"),
            Some(&task.id.0)
        )
    );

    let done = send(
        &f.handler,
        as_user("t-bob", Some("bearer:alice")),
        decision(&task, &req, true),
    )
    .await;
    println!(
        "after approval: {:?}; tool ran {} time(s) with {:?}",
        done.status.state,
        f.ran.load(Ordering::SeqCst),
        f.saw.lock().unwrap()
    );
    assert_eq!(
        f.ran.load(Ordering::SeqCst),
        1,
        "the approved call ran once"
    );
    assert_eq!(
        f.saw.lock().unwrap()[0],
        arguments,
        "it ran with the arguments the approver was shown"
    );
    assert_eq!(done.status.state, TaskState::Completed, "{done:?}");
}

#[tokio::test]
#[ignore = "needs a llama-server (A2A_LIVE_MODEL_URL)"]
async fn a_real_models_refused_call_never_runs() {
    let f = fixture();
    let (task, req) = parked(&f).await;
    let done = send(
        &f.handler,
        as_user("t-bob", Some("bearer:alice")),
        decision(&task, &req, false),
    )
    .await;
    println!(
        "after refusal: {:?}; tool ran {} time(s)",
        done.status.state,
        f.ran.load(Ordering::SeqCst)
    );
    assert_eq!(f.ran.load(Ordering::SeqCst), 0, "a refused call ran");
    assert_ne!(done.status.state, TaskState::InputRequired, "{done:?}");
}
