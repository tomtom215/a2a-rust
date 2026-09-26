// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Extended behaviour contract (identical in agent-rust/src/deep.rs):
//!   * a task stored in INPUT_REQUIRED + any follow-up -> artifact "Echo: <text>", COMPLETED
//!   * "ask:"   -> INPUT_REQUIRED with agent message "need more input"
//!   * "fail:"  -> FAILED with agent message "boom"
//!   * "msg:"   -> a direct Message reply "Reply: <text>" (no task)
//!   * "parts:" -> one artifact whose parts are the request's parts verbatim and whose
//!                 metadata is the request message's metadata; COMPLETED
//!   * "slow:"  -> five artifact chunks "c0".."c4" 300 ms apart (append after the first,
//!                 lastChunk on the last), stopping early when canceled; COMPLETED
//! Both agents run this hook before sending WORKING; every branch except
//! `msg:` sends WORKING itself, and `msg:` must not create a task.
use a2a::event::StreamResponse;
use a2a::*;
use a2a_server::ExecutorContext;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

type Tx = Sender<Result<StreamResponse, A2AError>>;

fn agent_msg(ctx: &ExecutorContext, text: &str) -> Message {
    let mut m = Message::new(Role::Agent, vec![Part::text(text)]);
    m.task_id = Some(ctx.task_id.clone());
    m.context_id = Some(ctx.context_id.clone());
    m
}

fn st(ctx: &ExecutorContext, state: TaskState, msg: Option<Message>) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: ctx.task_id.clone(),
        context_id: ctx.context_id.clone(),
        status: TaskStatus { state, message: msg, timestamp: Some(chrono::Utc::now()) },
        metadata: None,
    })
}

fn art(ctx: &ExecutorContext, id: &str, parts: Vec<Part>, meta: Option<std::collections::HashMap<String, serde_json::Value>>, append: Option<bool>, last: Option<bool>) -> StreamResponse {
    StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
        task_id: ctx.task_id.clone(),
        context_id: ctx.context_id.clone(),
        artifact: Artifact { artifact_id: id.into(), name: None, description: None, parts, metadata: meta, extensions: None },
        append,
        last_chunk: last,
        metadata: None,
    })
}

/// Returns true when the message was handled here.
pub async fn handle(ctx: &ExecutorContext, tx: &Tx, text: &str) -> bool {
    let working = || st(ctx, TaskState::Working, None);
    if ctx.stored_task.as_ref().is_some_and(|t| t.status.state == TaskState::InputRequired) {
        let _ = tx.send(Ok(working())).await;
        let _ = tx.send(Ok(art(ctx, "answer", vec![Part::text(format!("Echo: {text}"))], None, None, Some(true)))).await;
        let _ = tx.send(Ok(st(ctx, TaskState::Completed, None))).await;
        return true;
    }
    if text.starts_with("ask:") {
        let _ = tx.send(Ok(working())).await;
        let _ = tx.send(Ok(st(ctx, TaskState::InputRequired, Some(agent_msg(ctx, "need more input"))))).await;
        return true;
    }
    if text.starts_with("fail:") {
        let _ = tx.send(Ok(working())).await;
        let _ = tx.send(Ok(st(ctx, TaskState::Failed, Some(agent_msg(ctx, "boom"))))).await;
        return true;
    }
    if text.starts_with("msg:") {
        let mut m = Message::new(Role::Agent, vec![Part::text(format!("Reply: {text}"))]);
        m.context_id = Some(ctx.context_id.clone());
        let _ = tx.send(Ok(StreamResponse::Message(m))).await;
        return true;
    }
    if text.starts_with("parts:") {
        let msg = ctx.message.as_ref();
        let parts = msg.map(|m| m.parts.clone()).unwrap_or_default();
        let meta = msg.and_then(|m| m.metadata.clone());
        let _ = tx.send(Ok(working())).await;
        let _ = tx.send(Ok(art(ctx, "parts", parts, meta, None, Some(true)))).await;
        let _ = tx.send(Ok(st(ctx, TaskState::Completed, None))).await;
        return true;
    }
    if text.starts_with("slow:") {
        let _ = tx.send(Ok(working())).await;
        for i in 0..5 {
            if tx.send(Ok(art(ctx, "slow", vec![Part::text(format!("c{i}"))], None, Some(i > 0), Some(i == 4)))).await.is_err() {
                return true;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(300)) => {}
                _ = tx.closed() => return true,
            }
        }
        let _ = tx.send(Ok(st(ctx, TaskState::Completed, None))).await;
        return true;
    }
    false
}
