// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An approval bound to one call, end to end through the real handler
//! (adversarial review S7, 2026-11-12), composed with the review grant (A4) and
//! the run journal (S1).
//!
//! Alice asks the agent to move money. The policy says that call needs a
//! person's approval, so the task parks `input-required` with a digest of that
//! exact call. Bob, named by the gate, answers through a review grant, and the
//! run resumes from its journal and makes the call he was shown, once. The model
//! is scripted here; `tool_loop_approval_live.rs` has a real one.

#![cfg(feature = "tool-loop")]

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_server::approval::{ApprovalGate, REVIEW_FOR_HEADER};
use a2a_protocol_server::auth::caller_id;
use a2a_protocol_server::tool_loop::{
    ChatMessage, ChatRequest, InMemoryStepJournal, ModelError, ModelTurn, PolicyDecision,
    PolicyRequest, StepJournal, Tool, ToolCall, ToolError, ToolInvocation, ToolLoop,
    ToolLoopExecutor, ToolModel, ToolPolicy, ToolSet, call_digest,
};
use a2a_protocol_server::{
    BearerTokenAuthInterceptor, RequestHandler, RequestHandlerBuilder, SendMessageResult,
};
use a2a_protocol_types::approval::{ApprovalDecision, ApprovalRequest};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{Task, TaskState};
use serde_json::{Value, json};

/// Calls `transfer` on its first turn and answers on any turn that has a result.
struct Scripted;

impl ToolModel for Scripted {
    fn model_name(&self) -> &str {
        "scripted"
    }
    fn provider_name(&self) -> &str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        let result = request.messages.iter().rev().find_map(|m| match m {
            ChatMessage::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        });
        let turn = match result {
            Some(r) if r.contains("not approved") => ModelTurn::new("it was not approved", vec![]),
            Some(_) => ModelTurn::new("sent", vec![]),
            None => ModelTurn::new(
                "",
                vec![ToolCall::new("c1", "transfer", r#"{"service":"x"}"#)],
            ),
        };
        Box::pin(async move { Ok(turn) })
    }
}

struct Transfer(Arc<AtomicUsize>);

impl Tool for Transfer {
    fn name(&self) -> &'static str {
        "transfer"
    }
    fn description(&self) -> &'static str {
        "Moves money."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"service": {"type": "string"}}})
    }
    fn call<'a>(
        &'a self,
        _invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok("transferred".to_owned()) })
    }
}

struct NeedsApproval;

impl ToolPolicy for NeedsApproval {
    fn authorize<'a>(
        &'a self,
        _request: &'a PolicyRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = PolicyDecision> + Send + 'a>> {
        Box::pin(async { PolicyDecision::RequireApproval("move money".to_owned()) })
    }
}

fn handler(ran: &Arc<AtomicUsize>) -> RequestHandler {
    let mut set = ToolSet::new();
    set.add(Transfer(Arc::clone(ran))).unwrap();
    let journal: Arc<dyn StepJournal> = Arc::new(InMemoryStepJournal::new());
    let executor = ToolLoopExecutor::new(
        ToolLoop::new(Scripted, set)
            .with_policy(NeedsApproval)
            .with_journal(journal),
    );
    RequestHandlerBuilder::new(executor)
        .without_agent_card()
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
            ("t-alice", "alice"),
            ("t-bob", "bob"),
            ("t-carol", "carol"),
        ]))
        .with_approval_gate(
            ApprovalGate::new()
                .with_approvers([caller_id::bearer("bob")])
                .with_review_grants(),
        )
        .build()
        .unwrap()
}

fn as_user(token: &str, requester: Option<&str>) -> HashMap<String, String> {
    let mut h = HashMap::from([("authorization".to_owned(), format!("Bearer {token}"))]);
    if let Some(r) = requester {
        h.insert(REVIEW_FOR_HEADER.to_owned(), r.to_owned());
    }
    h
}

async fn send(
    h: &RequestHandler,
    who: HashMap<String, String>,
    m: Message,
) -> Result<Task, a2a_protocol_server::ServerError> {
    match h
        .on_send_message(MessageSendParams::new(m), false, Some(&who))
        .await?
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => Ok(t),
        other => panic!("a task: {other:?}"),
    }
}

fn ask() -> Message {
    Message::new(
        MessageId::new("m-ask"),
        MessageRole::User,
        vec![Part::text("send 500 to account 4417")],
    )
}

/// The decision `approve` makes on `request`, continuing `task`.
fn decision(
    task: &Task,
    request: &ApprovalRequest,
    approve: bool,
    digest: Option<&str>,
) -> Message {
    let mut d = if approve {
        ApprovalDecision::approve(request)
    } else {
        ApprovalDecision::deny(request)
    };
    if let Some(digest) = digest {
        d.digest = digest.to_owned();
    }
    let mut m = Message::user_text("a-1", "decided");
    m.task_id = Some(task.id.clone());
    m.context_id = Some(task.context_id.clone());
    d.attach(&mut m);
    m
}

fn answer_of(t: &Task) -> String {
    t.artifacts
        .as_ref()
        .and_then(|a| a.iter().find(|a| a.id.0 == "answer"))
        .and_then(|a| a.parts.first())
        .and_then(Part::text_content)
        .unwrap_or_default()
        .to_owned()
}

async fn parked(h: &RequestHandler) -> (Task, ApprovalRequest) {
    let task = send(h, as_user("t-alice", None), ask()).await.unwrap();
    assert_eq!(task.status.state, TaskState::InputRequired, "{task:?}");
    let req = ApprovalRequest::read(task.status.message.as_ref().unwrap())
        .unwrap()
        .expect("the request is on the status message");
    (task, req)
}

#[tokio::test]
async fn an_approval_for_the_call_runs_it_once_and_the_task_completes() {
    let ran = Arc::new(AtomicUsize::new(0));
    let h = handler(&ran);
    let (task, req) = parked(&h).await;
    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "it ran before it was approved"
    );
    // The digest the person is shown is the digest of the call as an action:
    // the tool, its arguments, and whose run it is.
    assert_eq!(
        req.digest,
        call_digest(
            "transfer",
            &json!({"service": "x"}),
            None,
            Some("bearer:alice"),
            Some(&task.id.0)
        )
    );
    assert_eq!(req.requested_by.as_deref(), Some("bearer:alice"));

    let done = send(
        &h,
        as_user("t-bob", Some("bearer:alice")),
        decision(&task, &req, true, None),
    )
    .await
    .unwrap();
    assert_eq!(done.status.state, TaskState::Completed, "{done:?}");
    assert_eq!(answer_of(&done), "sent");
    assert_eq!(ran.load(Ordering::SeqCst), 1, "the approved call ran once");
    let hers = h
        .on_get_task(
            TaskQueryParams::new(task.id.0.clone()),
            Some(&as_user("t-alice", None)),
        )
        .await
        .unwrap();
    assert_eq!(hers.status.state, TaskState::Completed);
}

#[tokio::test]
async fn a_refusal_completes_the_task_and_the_call_never_runs() {
    let ran = Arc::new(AtomicUsize::new(0));
    let h = handler(&ran);
    let (task, req) = parked(&h).await;
    let done = send(
        &h,
        as_user("t-bob", Some("bearer:alice")),
        decision(&task, &req, false, None),
    )
    .await
    .unwrap();
    assert_eq!(done.status.state, TaskState::Completed, "{done:?}");
    assert_eq!(answer_of(&done), "it was not approved");
    assert_eq!(ran.load(Ordering::SeqCst), 0);
}

/// An approval of another action, and an approval from someone the gate does
/// not admit, release nothing.
#[tokio::test]
async fn a_decision_for_another_action_or_from_another_caller_releases_nothing() {
    let ran = Arc::new(AtomicUsize::new(0));
    let h = handler(&ran);
    let (task, req) = parked(&h).await;

    // A digest of a different call (the amount shown was not this).
    let other = call_digest(
        "transfer",
        &json!({"service": "y"}),
        None,
        Some("bearer:alice"),
        Some(&task.id.0),
    );
    let wrong = send(
        &h,
        as_user("t-bob", Some("bearer:alice")),
        decision(&task, &req, true, Some(&other)),
    )
    .await;
    assert!(wrong.is_err(), "{wrong:?}");

    // Carol is authenticated and is no approver; Alice may not approve her own run.
    for (token, requester) in [("t-carol", Some("bearer:alice")), ("t-alice", None)] {
        let r = send(
            &h,
            as_user(token, requester),
            decision(&task, &req, true, None),
        )
        .await;
        assert!(r.is_err(), "{token}: {r:?}");
    }
    assert_eq!(ran.load(Ordering::SeqCst), 0, "nothing was released");
    let still = h
        .on_get_task(
            TaskQueryParams::new(task.id.0.clone()),
            Some(&as_user("t-alice", None)),
        )
        .await
        .unwrap();
    assert_eq!(still.status.state, TaskState::InputRequired);
}
