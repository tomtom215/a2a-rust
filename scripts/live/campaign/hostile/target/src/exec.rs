// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The misbehaving executor. The first text part of the message picks the
//! behaviour (`name` or `name:arg`), optionally preceded by `d<ms>/` which waits
//! that long after the first `working` event (so subscribers can attach):
//!
//! * `echo[:text]`: working, an artifact, completed.
//! * `hold`: working until cancelled (60 s at most), then returns Ok.
//! * `park`: working, then input-required.
//! * `panic[:n]`: n artifact events (default 0), then panics.
//! * `noterm[:n]`: working, n artifacts, returns Ok with no terminal state.
//! * `block`: working, then sleeps 1 h ignoring cancellation.
//! * `closeq[:n]`: working, n artifacts, then closes the queue and returns Ok.
//! * `afterterm[:n]`: completed, then n more artifacts after the terminal.
//! * `many:n`: n artifact events, then completed.
//! * `big:n`: one artifact with n bytes of text, then completed.
//! * `steer`: working; every steering message becomes an artifact `steer-<k>`
//!   carrying its text; a steering message `done` completes the task.
//! * `tick:<n>:<ms>`: n parts of artifact `tick`, `ms` apart, then completed; stops
//!   at once, writing nothing, when cancelled.
//! * `fail`: the `failed` state.
//! * `errret`: returns Err without a terminal state.

use std::time::Duration;

use a2a_protocol_server::EventEmitter;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::task::{TaskState, TaskStatus};

type Fut<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>>;

pub struct Exec;

fn arg(s: &str) -> Option<&str> {
    s.split_once(':').map(|(_, a)| a)
}

fn n(s: &str) -> usize {
    arg(s).and_then(|a| a.parse().ok()).unwrap_or(0)
}

async fn artifacts(emit: &EventEmitter<'_>, id: &str, count: usize) -> A2aResult<()> {
    for i in 0..count {
        emit.artifact(id, vec![Part::text(i.to_string())], Some(i > 0), Some(false))
            .await?;
    }
    Ok(())
}

impl AgentExecutor for Exec {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            let full = ctx.message.text().unwrap_or_default().to_owned();
            let (delay, text) = match full.strip_prefix('d').and_then(|r| r.split_once('/')) {
                Some((ms, rest)) if ms.parse::<u64>().is_ok() => {
                    (ms.parse::<u64>().unwrap_or(0), rest.to_owned())
                }
                _ => (0, full),
            };
            let kind = text.split(':').next().unwrap_or("");
            eprintln!("[{}] exec start {}", std::env::var("NAME").unwrap_or_default(), ctx.task_id);
            emit.status(TaskState::Working).await?;
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            match kind {
                "hold" => {
                    let _ = tokio::time::timeout(
                        Duration::from_secs(60),
                        ctx.cancellation_token.cancelled(),
                    )
                    .await;
                }
                "park" => emit.status(TaskState::InputRequired).await?,
                "panic" => {
                    artifacts(&emit, "p", n(&text)).await?;
                    panic!("hostile executor panic");
                }
                "noterm" => artifacts(&emit, "p", n(&text)).await?,
                "closeq" => {
                    artifacts(&emit, "p", n(&text)).await?;
                    queue.close().await?;
                }
                "block" => tokio::time::sleep(Duration::from_secs(3600)).await,
                "afterterm" => {
                    emit.status(TaskState::Completed).await?;
                    for i in 0..n(&text) {
                        let part = vec![Part::text(i.to_string())];
                        if let Err(e) = emit.artifact("late", part, Some(i > 0), Some(false)).await
                        {
                            eprintln!("afterterm write {i}: {e}");
                        }
                    }
                }
                "many" => {
                    artifacts(&emit, "m", n(&text)).await?;
                    emit.status(TaskState::Completed).await?;
                }
                "big" => {
                    emit.artifact("b", vec![Part::text("x".repeat(n(&text)))], None, None)
                        .await?;
                    emit.status(TaskState::Completed).await?;
                }
                // Peer- or model-supplied content a JSON store cannot hold.
                "nulart" => {
                    let r = emit
                        .artifact("n", vec![Part::text("a\u{0}b")], None, None)
                        .await;
                    eprintln!("nulart: artifact write answered {r:?}");
                    emit.status(TaskState::Completed).await?;
                }
                "nulstatus" | "badts" => {
                    let mut status = TaskStatus::new(TaskState::Completed);
                    if kind == "nulstatus" {
                        status.message = Some(Message::agent("m-nul", vec![Part::text("a\u{0}b")]));
                    } else {
                        status.timestamp = Some("0000-01-01T00:00:00Z".to_owned());
                    }
                    let ev = TaskStatusUpdateEvent::new(
                        ctx.task_id.clone(),
                        ctx.context_id.clone(),
                        status,
                    );
                    let r = queue.write(StreamResponse::StatusUpdate(ev)).await;
                    eprintln!("{kind}: write answered {r:?}");
                }
                "steer" => {
                    let Some(inbox) = ctx.inbox() else {
                        eprintln!("steer: this turn has no inbox");
                        return emit.status(TaskState::Failed).await;
                    };
                    let mut k = 0usize;
                    let deadline = tokio::time::sleep(Duration::from_secs(120));
                    tokio::pin!(deadline);
                    loop {
                        tokio::select! {
                            () = &mut deadline => break,
                            () = ctx.cancellation_token.cancelled() => return Ok(()),
                            m = inbox.recv() => {
                                let Some(s) = m else { break };
                                let t = s.message.text().unwrap_or_default().to_owned();
                                k += 1;
                                let id = format!("steer-{k}");
                                emit.artifact(&id, vec![Part::text(t.clone())], None, Some(true))
                                    .await?;
                                if t == "done" {
                                    break;
                                }
                            }
                        }
                    }
                    emit.status(TaskState::Completed).await?;
                }
                "tick" => {
                    let mut it = text.split(':').skip(1);
                    let n: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(10);
                    let ms: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(100);
                    for i in 0..n {
                        tokio::select! {
                            () = ctx.cancellation_token.cancelled() => return Ok(()),
                            () = tokio::time::sleep(Duration::from_millis(ms)) => {}
                        }
                        let part = vec![Part::text(i.to_string())];
                        emit.artifact("tick", part, Some(i > 0), Some(false)).await?;
                    }
                    emit.status(TaskState::Completed).await?;
                }
                "errret" => return Err(A2aError::internal("hostile executor error")),
                "fail" => emit.status(TaskState::Failed).await?,
                _ => {
                    let body = arg(&text).unwrap_or("ok").to_owned();
                    emit.artifact("echo", vec![Part::text(body)], None, None)
                        .await?;
                    emit.status(TaskState::Completed).await?;
                }
            }
            Ok(())
        })
    }
}
