// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The middle tier: an A2A agent whose work is delegating to other A2A agents.
//!
//! Its message is `batch:` followed by a JSON array of worker jobs. It runs them
//! across its worker pool with at most `max_inflight` outstanding, and answers
//! with one artifact — a JSON summary of what happened to every job.
//!
//! What a swarm needs from the layer below:
//!
//! * **Retry by class.** A child that fails [`FailureClass::Transient`] is
//!   re-sent once, to a *different* worker, with the fault removed. Any other
//!   class is final — the classification is what makes that a `match` rather
//!   than a guess about an error string.
//! * **Cancellation is not inherited.** A2A has no parent/child link between
//!   tasks, so cancelling this task does nothing to the tasks it created unless
//!   this code cancels each one. Until 2026-10-08 that cascade was written out
//!   here by hand; it is now [`Delegation`], which follows each child and
//!   cancels it when this task's cancellation token fires. The control arm
//!   detaches instead, to show what A2A does about children on its own.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use a2a_protocol_client::A2aClient;
use a2a_protocol_client::delegation::{self, Delegation};
use a2a_protocol_server::{AgentExecutor, EventEmitter, EventQueueWriter, RequestContext};
use a2a_protocol_types::failure::FailureClass;
use a2a_protocol_types::{A2aResult, Message, MessageSendParams, Part, TaskState};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub struct Supervisor {
    pub workers: Arc<Vec<Arc<A2aClient>>>,
    pub max_inflight: usize,
    pub next: Arc<AtomicUsize>,
    /// `false` turns off the hand-written cancel fan-out: the control arm of the
    /// cancel scenario, showing what A2A does about children on its own.
    pub propagate: bool,
}

/// What became of one job, after any retry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Completed,
    Failed(FailureClass),
    Canceled,
    /// The transport failed, or the child's stream ended without a terminal state.
    Lost,
}

struct Ran {
    outcome: Outcome,
    retried: bool,
    micros: u128,
}

/// Sends one job to one worker and follows it to a terminal state, cancelling
/// the child if `stop` fires first.
async fn run_once(
    worker: &Arc<A2aClient>,
    job: &str,
    stop: &CancellationToken,
    propagate: bool,
) -> Outcome {
    let params = MessageSendParams::new(Message::user(
        uuid::Uuid::new_v4().to_string(),
        vec![Part::text(job)],
    ));
    let child = match Delegation::start(Arc::clone(worker), params).await {
        Ok(d) => d,
        Err(e) => {
            if std::env::var_os("SWARM_DEBUG").is_some() {
                eprintln!("stream_message failed: {e}");
            }
            return Outcome::Lost;
        }
    };
    let done = if propagate {
        child.wait(stop.cancelled()).await
    } else {
        // The control arm: on the stop signal, walk away from the child
        // without cancelling it, as an orchestrator that wrote no cascade
        // would.
        let mut child = child;
        loop {
            tokio::select! {
                ev = child.next_event() => {
                    if child.is_settled() {
                        break child.wait(std::future::pending()).await;
                    }
                    if ev.is_none() {
                        let _ = child.detach();
                        return Outcome::Lost;
                    }
                }
                () = stop.cancelled() => {
                    let _ = child.detach();
                    return Outcome::Canceled;
                }
            }
        }
    };
    match done.outcome {
        delegation::Outcome::Completed(_) => Outcome::Completed,
        delegation::Outcome::Failed { class, .. } => Outcome::Failed(class),
        // The parent stopped: the child was cancelled, or asked to be.
        delegation::Outcome::Canceled(_)
        | delegation::Outcome::CancelRequested(_)
        | delegation::Outcome::CancelFailed(_)
        | delegation::Outcome::Unreachable => Outcome::Canceled,
        _ => Outcome::Lost,
    }
}

async fn run_job(
    workers: Arc<Vec<Arc<A2aClient>>>,
    start: usize,
    job: String,
    stop: CancellationToken,
    propagate: bool,
) -> Ran {
    let t0 = Instant::now();
    let first = run_once(&workers[start % workers.len()], &job, &stop, propagate).await;
    let (outcome, retried) = match first {
        Outcome::Failed(c) if c.is_retryable() && !stop.is_cancelled() => {
            let healed = job.replacen("flaky:", "sleep:", 1);
            (
                run_once(
                    &workers[(start + 1) % workers.len()],
                    &healed,
                    &stop,
                    propagate,
                )
                .await,
                true,
            )
        }
        o => (o, false),
    };
    Ran {
        outcome,
        retried,
        micros: t0.elapsed().as_micros(),
    }
}

impl AgentExecutor for Supervisor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            let text = ctx.message.text().unwrap_or_default();
            let Some(jobs) = text
                .strip_prefix("batch:")
                .and_then(|j| serde_json::from_str::<Vec<String>>(j).ok())
            else {
                return emit
                    .fail(FailureClass::InvalidRequest, "expected batch:<json array>")
                    .await;
            };
            emit.status(TaskState::Working).await?;

            let gate = Arc::new(Semaphore::new(self.max_inflight));
            let stop = ctx.cancellation_token.clone();
            let mut set = tokio::task::JoinSet::new();
            for job in jobs {
                let (gate, workers, stop) = (gate.clone(), self.workers.clone(), stop.clone());
                let start = self.next.fetch_add(1, Ordering::Relaxed);
                let propagate = self.propagate;
                set.spawn(async move {
                    let _permit = gate
                        .acquire_owned()
                        .await
                        .expect("semaphore is never closed");
                    run_job(workers, start, job, stop, propagate).await
                });
            }
            let mut tally = serde_json::Map::new();
            let mut lat: Vec<u128> = Vec::new();
            let mut retried = 0u64;
            while let Some(r) = set.join_next().await {
                let r = r.expect("job task does not panic");
                let key = match r.outcome {
                    Outcome::Completed => "completed".to_owned(),
                    Outcome::Failed(c) => format!("failed:{c:?}"),
                    Outcome::Canceled => "canceled".to_owned(),
                    Outcome::Lost => "lost".to_owned(),
                };
                let n = tally.entry(key).or_insert(serde_json::json!(0));
                *n = serde_json::json!(n.as_u64().unwrap_or(0) + 1);
                retried += u64::from(r.retried);
                if r.outcome == Outcome::Completed {
                    lat.push(r.micros);
                }
            }
            if stop.is_cancelled() {
                // The handler's default `cancel` emits the terminal Canceled.
                return Ok(());
            }
            let summary =
                serde_json::json!({ "outcomes": tally, "retried": retried, "job_latency_us": lat });
            emit.artifact(
                "summary",
                vec![Part::text(summary.to_string())],
                None,
                Some(true),
            )
            .await?;
            emit.status(TaskState::Completed).await
        })
    }
}
