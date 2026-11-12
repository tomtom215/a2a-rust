// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A reviewer reaches a task that waits for their decision, and nothing else of
//! the requester's (adversarial review A4, 2026-11-12).
//!
//! Before, an approver reached the requester's task only if every caller
//! shared every task (`share_tasks_across_callers`), or if the requester
//! answered their own run. Alice asks; Bob, named by the gate, answers; Carol
//! is authenticated and is not an approver.

use std::collections::HashMap;

use a2a_protocol_types::approval::{ApprovalDecision, ApprovalRequest};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{
    CancelTaskParams, ListTasksParams, MessageSendParams, TaskQueryParams,
};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{Task, TaskState};

use crate::approval::{ApprovalGate, REVIEW_FOR_HEADER};
use crate::auth::caller_id;
use crate::builder::RequestHandlerBuilder;
use crate::call_context::CallContext;
use crate::error::{ServerError, ServerResult};
use crate::{
    BearerTokenAuthInterceptor, EventEmitter, RequestHandler, SendMessageResult, agent_executor,
};

/// Asks before refunding; on the next run, says who approved.
struct Refunds;
agent_executor!(Refunds, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    if ctx.stored_task.is_none() {
        let req = ApprovalRequest::new("req-1", "Refund EUR 40", "sha256:aa");
        emit.request_approval(req, "May I refund?").await?;
        return Ok(());
    }
    let seen = ctx.approval().map_or_else(
        || "no approval".to_owned(),
        |a| format!("{:?} by {}", a.decision.decision, a.approver),
    );
    emit.artifact("result", vec![Part::text(seen)], None, Some(true))
        .await?;
    emit.status(TaskState::Completed).await
});

fn builder(gate: ApprovalGate) -> RequestHandlerBuilder {
    RequestHandlerBuilder::new(Refunds)
        .without_agent_card()
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
            ("t-alice", "alice"),
            ("t-bob", "bob"),
            ("t-carol", "carol"),
        ]))
        .with_approval_gate(gate)
}

/// Bob holds the grant. Tasks are isolated by caller, as they are by default.
fn handler() -> RequestHandler {
    builder(
        ApprovalGate::new()
            .with_approvers([caller_id::bearer("bob")])
            .with_review_grants(),
    )
    .build()
    .expect("isolated by caller, with a grant")
}

/// `token`'s headers, naming `requester` when a reviewer does.
fn headers(token: &str, requester: Option<&str>) -> HashMap<String, String> {
    let mut h = HashMap::from([("authorization".to_owned(), format!("Bearer {token}"))]);
    if let Some(r) = requester {
        h.insert(REVIEW_FOR_HEADER.to_owned(), r.to_owned());
    }
    h
}

async fn send(h: &RequestHandler, who: HashMap<String, String>, m: Message) -> ServerResult<Task> {
    match h
        .on_send_message(MessageSendParams::new(m), false, Some(&who))
        .await?
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => Ok(t),
        other => panic!("expected a task, got {other:?}"),
    }
}

async fn get(h: &RequestHandler, who: HashMap<String, String>, id: &str) -> ServerResult<Task> {
    h.on_get_task(TaskQueryParams::new(id.to_owned()), Some(&who))
        .await
}

fn ask() -> Message {
    Message::new(
        MessageId::new("m-ask"),
        MessageRole::User,
        vec![Part::text("refund 1182")],
    )
}

/// Alice's task, waiting, with the decision that answers it.
async fn asked(h: &RequestHandler) -> (Task, Message) {
    let task = send(h, headers("t-alice", None), ask()).await.unwrap();
    assert_eq!(task.status.state, TaskState::InputRequired);
    let req = ApprovalRequest::read(task.status.message.as_ref().unwrap())
        .unwrap()
        .unwrap();
    let mut decision = Message::user_text("a-1", "approved");
    decision.task_id = Some(task.id.clone());
    decision.context_id = Some(task.context_id.clone());
    ApprovalDecision::approve(&req).attach(&mut decision);
    (task, decision)
}

fn denied<T: std::fmt::Debug>(r: &ServerResult<T>, says: &str) {
    match r {
        Err(ServerError::Protocol(e)) => assert!(e.message.contains(says), "{}", e.message),
        other => panic!("expected a refusal ({says}), got {other:?}"),
    }
}

fn not_found<T: std::fmt::Debug>(r: &ServerResult<T>) {
    assert!(matches!(r, Err(ServerError::TaskNotFound(_))), "{r:?}");
}

/// The whole path: Bob sees the waiting task, answers it, and the run
/// continues in Alice's partition, where Alice finds it done and Bob does not.
#[tokio::test]
async fn a_reviewer_reads_and_answers_the_task_waiting_for_them() {
    let h = handler();
    let (task, decision) = asked(&h).await;
    let bob = || headers("t-bob", Some("bearer:alice"));

    let seen = get(&h, bob(), &task.id.0).await.unwrap();
    assert_eq!(seen.status.state, TaskState::InputRequired);

    let done = send(&h, bob(), decision).await.unwrap();
    assert_eq!(done.status.state, TaskState::Completed);
    let text = done.artifacts.as_ref().unwrap()[0].parts[0]
        .text_content()
        .unwrap();
    assert_eq!(text, "Approve by bearer:bob");

    // The task is Alice's: she finds it finished, and Bob's own records hold none.
    let hers = get(&h, headers("t-alice", None), &task.id.0).await.unwrap();
    assert_eq!(hers.status.state, TaskState::Completed);
    let bobs = h
        .on_list_tasks(ListTasksParams::default(), Some(&headers("t-bob", None)))
        .await
        .unwrap();
    assert_eq!(bobs.tasks.len(), 0, "the approval was not stored under Bob");
    not_found(&get(&h, headers("t-bob", None), &task.id.0).await);
}

/// A reviewer reads a task only while it waits for a decision.
#[tokio::test]
async fn a_task_that_is_not_waiting_is_not_found_to_the_reviewer() {
    let h = handler();
    let (task, decision) = asked(&h).await;
    send(&h, headers("t-bob", Some("bearer:alice")), decision)
        .await
        .unwrap();

    // Finished now: Alice can read it, Bob cannot.
    get(&h, headers("t-alice", None), &task.id.0).await.unwrap();
    not_found(&get(&h, headers("t-bob", Some("bearer:alice")), &task.id.0).await);
    // Nor one that was never there, nor one of a requester with none.
    not_found(&get(&h, headers("t-bob", Some("bearer:alice")), "no-such-task").await);
    not_found(&get(&h, headers("t-bob", Some("bearer:mallory")), &task.id.0).await);
}

/// "Waiting" means input-required *and* carrying the request: a task in another
/// state is not shown to a reviewer even if its message still holds one.
#[test]
fn only_an_input_required_task_with_the_request_is_shown() {
    use a2a_protocol_types::task::{ContextId, TaskId, TaskStatus};
    let h = handler();
    let review = CallContext::new("GetTask")
        .with_caller_identity("bearer:bob".to_owned())
        .with_http_header(REVIEW_FOR_HEADER, "bearer:alice");
    let own = CallContext::new("GetTask").with_caller_identity("bearer:alice".to_owned());
    let mut note = Message::agent_text("q", "approve?");
    ApprovalRequest::new("req-1", "Refund EUR 40", "sha256:aa").attach(&mut note);
    let task = |state: TaskState, message: Option<Message>| {
        let mut status = TaskStatus::new(state);
        status.message = message;
        Task::new(TaskId::new("t-1"), ContextId::new("c-1"), status)
    };
    let shown = |t: &Task| h.ensure_review_sees_a_waiting_task(&review, t).is_ok();

    assert!(shown(&task(TaskState::InputRequired, Some(note.clone()))));
    assert!(
        !shown(&task(TaskState::Working, Some(note.clone()))),
        "Working"
    );
    assert!(
        !shown(&task(TaskState::Completed, Some(note.clone()))),
        "Completed"
    );
    assert!(!shown(&task(TaskState::InputRequired, None)), "no message");
    let plain = Some(Message::agent_text("q", "anything?"));
    assert!(!shown(&task(TaskState::InputRequired, plain)), "no request");
    // A call that is not a review is not held to any of this.
    assert!(
        h.ensure_review_sees_a_waiting_task(&own, &task(TaskState::Working, None))
            .is_ok()
    );
}

/// Bob may send an answer and nothing else into Alice's task: not a message
/// that would continue it, not one that would start another.
#[tokio::test]
async fn a_reviewer_can_send_an_answer_and_nothing_else() {
    let h = handler();
    let (task, _) = asked(&h).await;
    let bob = || headers("t-bob", Some("bearer:alice"));

    let mut continues = Message::user_text("m-2", "carry on and refund 9000");
    continues.task_id = Some(task.id.clone());
    continues.context_id = Some(task.context_id.clone());
    denied(&send(&h, bob(), continues).await, "is not one");

    // No task named: the executor would start a new one in Alice's partition.
    denied(&send(&h, bob(), ask()).await, "is not one");

    let hers = h
        .on_list_tasks(ListTasksParams::default(), Some(&headers("t-alice", None)))
        .await
        .unwrap();
    assert_eq!(hers.tasks.len(), 1, "nothing was added to Alice's tasks");
    assert_eq!(hers.tasks[0].status.state, TaskState::InputRequired);
}

/// Anyone but a named approver is refused the grant, and without the header
/// everyone is where they were: in their own partition.
#[tokio::test]
async fn only_a_named_approver_holds_the_grant() {
    let h = handler();
    let (task, decision) = asked(&h).await;

    let carol = || headers("t-carol", Some("bearer:alice"));
    denied(&get(&h, carol(), &task.id.0).await, "holds no review grant");
    denied(
        &send(&h, carol(), decision.clone()).await,
        "holds no review grant",
    );
    // Alice, naming herself, is no approver either.
    denied(
        &get(&h, headers("t-alice", Some("bearer:alice")), &task.id.0).await,
        "holds no review grant",
    );
    // Without the header, Carol and Bob are in their own partitions.
    not_found(&get(&h, headers("t-carol", None), &task.id.0).await);
    not_found(&get(&h, headers("t-bob", None), &task.id.0).await);
    not_found(&send(&h, headers("t-bob", None), decision).await);
}

/// The grant covers `GetTask` and the two sends, and no other method.
#[tokio::test]
async fn the_grant_covers_three_methods() {
    let h = handler();
    let bob = || headers("t-bob", Some("bearer:alice"));
    let (task, _) = asked(&h).await;

    denied(
        &h.on_list_tasks(ListTasksParams::default(), Some(&bob()))
            .await,
        "holds no review grant",
    );
    denied(
        &h.on_cancel_task(CancelTaskParams::new(task.id.0.clone()), Some(&bob()))
            .await,
        "holds no review grant",
    );
    let hers = get(&h, headers("t-alice", None), &task.id.0).await.unwrap();
    assert_eq!(hers.status.state, TaskState::InputRequired, "not canceled");

    let ctx = |method: &str| {
        CallContext::new(method)
            .with_caller_identity("bearer:bob".to_owned())
            .with_http_header(REVIEW_FOR_HEADER, "bearer:alice")
    };
    for (method, covered) in [
        ("GetTask", true),
        ("SendMessage", true),
        ("SendStreamingMessage", true),
        ("ListTasks", false),
        ("CancelTask", false),
        ("SubscribeToTask", false),
        ("CreateTaskPushNotificationConfig", false),
        ("GetExtendedAgentCard", false),
    ] {
        let scope = h.review_scope(&ctx(method));
        assert_eq!(scope.is_ok(), covered, "{method}: {scope:?}");
        if covered {
            assert_eq!(scope.unwrap().as_deref(), Some("bearer:alice"), "{method}");
        }
    }
    // An empty requester names nobody.
    let empty = ctx("GetTask").with_http_header(REVIEW_FOR_HEADER, "");
    assert!(h.review_scope(&empty).is_err());
    // No header: no scope, and not a refusal.
    let plain = CallContext::new("ListTasks").with_caller_identity("bearer:bob".to_owned());
    assert_eq!(h.review_scope(&plain).unwrap(), None);
}

/// A grant does not waive the gate's other rules: the requester still cannot
/// approve their own run, even when named an approver.
#[tokio::test]
async fn a_grant_does_not_let_the_requester_approve_their_own_run() {
    let h = builder(
        ApprovalGate::new()
            .with_approvers([caller_id::bearer("alice"), caller_id::bearer("bob")])
            .with_review_grants(),
    )
    .build()
    .unwrap();
    let (_, decision) = asked(&h).await;
    let own = send(&h, headers("t-alice", Some("bearer:alice")), decision).await;
    denied(&own, "someone other than the caller whose run asked");
}

/// A quorum can be met under isolation with grants: two reviewers each answer,
/// the executor runs once, after the second.
#[tokio::test]
async fn a_quorum_is_met_by_reviewers_under_isolation() {
    let h = builder(
        ApprovalGate::new()
            .with_approvers([caller_id::bearer("bob"), caller_id::bearer("carol")])
            .with_quorum(2)
            .with_review_grants(),
    )
    .build()
    .expect("a quorum with grants is satisfiable under isolation");
    let (task, decision) = asked(&h).await;

    let first = send(&h, headers("t-bob", Some("bearer:alice")), decision.clone())
        .await
        .unwrap();
    assert_eq!(first.status.state, TaskState::InputRequired, "one of two");
    let mut second = decision;
    second.id = MessageId::new("a-2");
    let done = send(&h, headers("t-carol", Some("bearer:alice")), second)
        .await
        .unwrap();
    assert_eq!(done.status.state, TaskState::Completed);
    let hers = get(&h, headers("t-alice", None), &task.id.0).await.unwrap();
    assert_eq!(hers.status.state, TaskState::Completed);
}

/// A grant is for the callers `with_approvers` names; one that names none is
/// refused at `build()`, and a gate without grants is refused as before.
#[test]
fn a_grant_needs_a_named_approver_and_the_refusals_name_it() {
    let Err(ServerError::Config(why)) = builder(ApprovalGate::new().with_review_grants()).build()
    else {
        panic!("a grant to everyone is not narrow");
    };
    assert!(why.contains("with_review_grants names no one"), "{why}");

    let Err(ServerError::Config(why)) =
        builder(ApprovalGate::new().with_approvers(["bearer:bob"])).build()
    else {
        panic!("a gate that needs a second caller is refused under isolation");
    };
    assert!(why.contains("ApprovalGate::with_review_grants()"), "{why}");

    let Err(ServerError::Config(why)) = builder(
        ApprovalGate::new()
            .with_approvers(["bearer:bob", "bearer:carol"])
            .with_quorum(2),
    )
    .build() else {
        panic!("a quorum under isolation is refused without grants");
    };
    assert!(why.contains("with_review_grants()"), "{why}");
}

/// With the audit log on, an answer given through a grant says so, and names
/// the reviewer, not the requester.
#[cfg(feature = "audit")]
#[tokio::test]
async fn an_answer_given_through_a_grant_is_audited_as_one() {
    use crate::audit::{AuditLog, InMemoryAuditStore};
    let log = std::sync::Arc::new(AuditLog::new(
        std::sync::Arc::new(InMemoryAuditStore::new()),
    ));
    let h = builder(
        ApprovalGate::new()
            .with_approvers([caller_id::bearer("bob")])
            .with_review_grants(),
    )
    .with_audit(std::sync::Arc::clone(&log))
    .build()
    .unwrap();
    let (_, decision) = asked(&h).await;
    send(&h, headers("t-bob", Some("bearer:alice")), decision)
        .await
        .unwrap();

    let records = log.export("").await.unwrap();
    let approval = records
        .iter()
        .find(|r| r.kind == "approval")
        .expect("an approval record");
    assert_eq!(approval.detail["reviewFor"], "bearer:alice");
    assert_eq!(
        approval.actor.as_ref().map(|a| a.subject.as_str()),
        Some("bearer:bob")
    );
}
