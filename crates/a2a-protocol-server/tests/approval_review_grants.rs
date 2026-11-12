// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A reviewer's narrow grant, over a real HTTP+JSON server (adversarial review
//! A4, 2026-11-12).
//!
//! Alice asks, Bob answers, Carol is authenticated and no approver. Tasks are
//! isolated by caller, as they are by default; the grant is what lets Bob in,
//! to a task that waits for him and to nothing else of Alice's.

use std::sync::Arc;

use a2a_protocol_server::approval::{ApprovalGate, REVIEW_FOR_HEADER};
use a2a_protocol_server::auth::caller_id;
use a2a_protocol_server::dispatch::RestDispatcher;
use a2a_protocol_server::serve::Server;
use a2a_protocol_server::{
    BearerTokenAuthInterceptor, EventEmitter, RequestHandlerBuilder, agent_executor,
};
use a2a_protocol_types::approval::{ApprovalDecision, ApprovalRequest};
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::task::{Task, TaskState};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;

struct Refunds;
agent_executor!(Refunds, |ctx, q| async {
    let emit = EventEmitter::new(ctx, q);
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

async fn server() -> std::net::SocketAddr {
    let handler = RequestHandlerBuilder::new(Refunds)
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
        .unwrap();
    let bound = Server::bind("127.0.0.1:0").await.unwrap();
    let addr = bound.local_addr().unwrap();
    tokio::spawn(bound.serve_with_shutdown(
        RestDispatcher::new(Arc::new(handler)),
        std::future::pending(),
    ));
    addr
}

async fn call(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    token: &str,
    requester: Option<&str>,
    body: Option<String>,
) -> (u16, serde_json::Value) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let mut req = hyper::Request::builder()
        .method(method)
        .uri(format!("http://{addr}{path}"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .header("authorization", format!("Bearer {token}"));
    if let Some(r) = requester {
        req = req.header(REVIEW_FOR_HEADER, r);
    }
    let resp = client
        .request(
            req.body(Full::new(Bytes::from(body.unwrap_or_default())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn task_of(v: &serde_json::Value) -> Task {
    serde_json::from_value(v.get("task").unwrap_or(v).clone()).expect("a task")
}

#[tokio::test]
async fn a_reviewer_answers_one_waiting_task_over_http_and_reaches_nothing_else() {
    let addr = server().await;
    let ask = serde_json::json!({
        "message": {"messageId": "m-ask", "role": "ROLE_USER", "parts": [{"text": "refund 1182"}]}
    });
    let (status, v) = call(
        addr,
        "POST",
        "/message:send",
        "t-alice",
        None,
        Some(ask.to_string()),
    )
    .await;
    assert_eq!(status, 200, "{v}");
    let asked = task_of(&v);
    assert_eq!(asked.status.state, TaskState::InputRequired);
    let id = &asked.id.0;
    let path = format!("/tasks/{id}");

    // Without the header Bob is in his own partition: Alice's task is not there.
    let (status, _) = call(addr, "GET", &path, "t-bob", None, None).await;
    assert_eq!(status, 404);
    // Carol names Alice and holds no grant.
    let (status, _) = call(addr, "GET", &path, "t-carol", Some("bearer:alice"), None).await;
    assert_eq!(status, 403);
    // Bob, with the header, sees the task that waits for him.
    let (status, v) = call(addr, "GET", &path, "t-bob", Some("bearer:alice"), None).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(task_of(&v).status.state, TaskState::InputRequired);

    // He may not send Alice's task anything but an answer.
    let other = serde_json::json!({"message": {
        "messageId": "m-2", "role": "ROLE_USER", "taskId": id, "parts": [{"text": "refund 9000"}]
    }});
    let (status, _) = call(
        addr,
        "POST",
        "/message:send",
        "t-bob",
        Some("bearer:alice"),
        Some(other.to_string()),
    )
    .await;
    assert_eq!(status, 403);

    // Or answer for someone he is not reviewing for.
    let req = ApprovalRequest::read(asked.status.message.as_ref().unwrap())
        .unwrap()
        .unwrap();
    let mut decision = Message::user_text("a-1", "approved");
    decision.task_id = Some(asked.id.clone());
    decision.context_id = Some(asked.context_id.clone());
    ApprovalDecision::approve(&req).attach(&mut decision);
    let body = serde_json::json!({ "message": decision }).to_string();
    let (status, _) = call(
        addr,
        "POST",
        "/message:send",
        "t-bob",
        Some("bearer:mallory"),
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, 404);

    let (status, v) = call(
        addr,
        "POST",
        "/message:send",
        "t-bob",
        Some("bearer:alice"),
        Some(body),
    )
    .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(task_of(&v).status.state, TaskState::Completed);

    // Done: Alice reads it; Bob, who could read it while it waited, cannot now.
    let (status, v) = call(addr, "GET", &path, "t-alice", None, None).await;
    assert_eq!(status, 200);
    assert_eq!(task_of(&v).status.state, TaskState::Completed);
    let (status, _) = call(addr, "GET", &path, "t-bob", Some("bearer:alice"), None).await;
    assert_eq!(status, 404);
    // And the grant never covered listing or canceling.
    let (status, _) = call(addr, "GET", "/tasks", "t-bob", Some("bearer:alice"), None).await;
    assert_eq!(status, 403);
    let (status, _) = call(
        addr,
        "POST",
        &format!("/tasks/{id}:cancel"),
        "t-bob",
        Some("bearer:alice"),
        Some("{}".to_owned()),
    )
    .await;
    assert_eq!(status, 403);
}
