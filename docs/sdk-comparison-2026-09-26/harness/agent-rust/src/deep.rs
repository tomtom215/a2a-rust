// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Extended behaviour contract (identical in agent-rs/src/deep.rs):
//!   * a task stored in INPUT_REQUIRED + any follow-up -> artifact "Echo: <text>", COMPLETED
//!   * "ask:"   -> INPUT_REQUIRED with agent message "need more input"
//!   * "fail:"  -> FAILED with agent message "boom"
//!   * "msg:"   -> a direct Message reply "Reply: <text>" (no task)
//!   * "parts:" -> one artifact whose parts are the request's parts verbatim and whose
//!                 metadata is the request message's metadata; COMPLETED
//!   * "slow:"  -> five artifact chunks "c0".."c4" 300 ms apart (append after the first,
//!                 lastChunk on the last), stopping early when canceled; COMPLETED
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::request_context::RequestContext;
use a2a_protocol_sdk::server::streaming::EventQueueWriter;
use a2a_protocol_sdk::types::{Artifact, ArtifactId, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_sdk::types::task::{ContextId, TaskStatus};
use std::time::Duration;

fn agent_msg(ctx: &RequestContext, text: &str) -> Message {
    let mut m = Message::agent(uuid_like(), vec![Part::text(text)]);
    m.task_id = Some(ctx.task_id.clone());
    m.context_id = Some(ContextId::new(ctx.context_id.clone()));
    m
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!("agent-msg-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

async fn status(ctx: &RequestContext, q: &dyn EventQueueWriter, state: TaskState, text: &str) -> A2aResult<()> {
    let mut st = TaskStatus::new(state);
    st.message = Some(agent_msg(ctx, text));
    q.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: ctx.task_id.clone(),
        context_id: ContextId::new(ctx.context_id.clone()),
        status: st,
        metadata: None,
    }))
    .await
}

pub async fn handle(
    ctx: &RequestContext,
    q: &dyn EventQueueWriter,
    emit: &EventEmitter<'_>,
    text: &str,
) -> Option<A2aResult<()>> {
    // Every branch but `msg:` sends WORKING first, as agent-rs/src/deep.rs does.
    if ctx.stored_task.as_ref().is_some_and(|t| t.status.state == TaskState::InputRequired) {
        return Some(async {
            emit.status(TaskState::Working).await?;
            emit.artifact("answer", vec![Part::text(format!("Echo: {text}"))], None, Some(true)).await?;
            emit.status(TaskState::Completed).await
        }.await);
    }
    if text.starts_with("ask:") {
        if let Err(e) = emit.status(TaskState::Working).await { return Some(Err(e)) }
        return Some(status(ctx, q, TaskState::InputRequired, "need more input").await);
    }
    if text.starts_with("fail:") {
        if let Err(e) = emit.status(TaskState::Working).await { return Some(Err(e)) }
        return Some(status(ctx, q, TaskState::Failed, "boom").await);
    }
    if text.starts_with("msg:") {
        let mut m = Message::agent(uuid_like(), vec![Part::text(format!("Reply: {text}"))]);
        m.context_id = Some(ContextId::new(ctx.context_id.clone()));
        return Some(q.write(StreamResponse::Message(m)).await);
    }
    if text.starts_with("parts:") {
        let mut art = Artifact::new("parts", ctx.message.parts.clone());
        art.metadata = ctx.message.metadata.clone();
        return Some(async {
            emit.status(TaskState::Working).await?;
            q.write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                artifact: art,
                append: None,
                last_chunk: Some(true),
                metadata: None,
            }))
            .await?;
            emit.status(TaskState::Completed).await
        }.await);
    }
    if text.starts_with("slow:") {
        return Some(async {
            emit.status(TaskState::Working).await?;
            for i in 0..5 {
                if emit.is_cancelled() {
                    return Ok(());
                }
                emit.artifact("slow", vec![Part::text(format!("c{i}"))], Some(i > 0), Some(i == 4)).await?;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(300)) => {}
                    _ = ctx.cancellation_token.cancelled() => return Ok(()),
                }
            }
            emit.status(TaskState::Completed).await
        }.await);
    }
    None
}
