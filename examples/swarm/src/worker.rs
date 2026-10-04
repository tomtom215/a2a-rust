// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The leaf agent. A job is the message text:
//!
//! | text | behaviour |
//! |---|---|
//! | `sleep:<ms>` | works for `ms`, honouring cancellation, then completes |
//! | `flaky:<ms>` | fails at once with [`FailureClass::Transient`] — the retry probe |
//! | `bad:<ms>` | fails at once with [`FailureClass::InvalidRequest`] — must not be retried |
//! | `llm:<prompt>` | asks the model and returns its answer as the artifact |
//!
//! `live` counts executions in flight. It is decremented by a drop guard, so it
//! falls on every exit path including cancellation — which is what lets the
//! cancel scenario measure quiescence instead of inferring it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use a2a_protocol_server::{AgentExecutor, EventEmitter, EventQueueWriter, RequestContext};
use a2a_protocol_types::failure::FailureClass;
use a2a_protocol_types::{A2aResult, Part, TaskState};

use crate::llm::Llm;

pub struct Worker {
    pub live: Arc<AtomicI64>,
    pub llm: Option<Llm>,
}

struct LiveGuard(Arc<AtomicI64>);
impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn millis(arg: &str) -> u64 {
    arg.parse().unwrap_or(0)
}

impl AgentExecutor for Worker {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.live.fetch_add(1, Ordering::SeqCst);
            let _guard = LiveGuard(self.live.clone());
            let emit = EventEmitter::new(ctx, queue);
            let text = ctx.message.text().unwrap_or_default().to_owned();
            let (kind, arg) = text.split_once(':').unwrap_or((text.as_str(), ""));
            emit.status(TaskState::Working).await?;
            match kind {
                "flaky" => {
                    return emit
                        .fail(FailureClass::Transient, "injected transient fault")
                        .await;
                }
                "bad" => {
                    return emit
                        .fail(FailureClass::InvalidRequest, "injected bad request")
                        .await;
                }
                "llm" => {
                    let Some(llm) = &self.llm else {
                        return emit
                            .fail(FailureClass::InvalidRequest, "no model configured")
                            .await;
                    };
                    let answer = tokio::select! {
                        r = llm.complete(arg, 48) => r,
                        () = ctx.cancellation_token.cancelled() => return Ok(()),
                    };
                    match answer {
                        Ok(a) => {
                            emit.artifact("answer", vec![Part::text(a)], None, Some(true))
                                .await?
                        }
                        Err(e) => return emit.fail(FailureClass::Transient, e).await,
                    }
                }
                _ => {
                    tokio::select! {
                        () = tokio::time::sleep(Duration::from_millis(millis(arg))) => {}
                        () = ctx.cancellation_token.cancelled() => return Ok(()),
                    }
                    emit.artifact(
                        "result",
                        vec![Part::text(format!("done {text}"))],
                        None,
                        Some(true),
                    )
                    .await?;
                }
            }
            emit.status(TaskState::Completed).await
        })
    }
}
