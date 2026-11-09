// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `C_MODE=coordinator`: delegates each message to the worker at
//! `$C_WORKER_URL` with `Delegation::start_budgeted`, granting it what is left
//! of the `tokens` meter (and the time) of the budget it was given, and relays
//! what the worker did. It is `examples/live-model`'s coordinator with its
//! findings on the tool log (`DELEGATION ...`), so a driver can see how the
//! delegation ended without reading the SDK's types.
//!
//! `$C_GRANT_METER` is the amount of `tokens` it asks the grant for
//! (default `u64::MAX`: all that is left, as `live-model` does).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::client::A2aClient;
use a2a_protocol_sdk::client::delegation::{Delegation, Outcome};
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::executor::AgentExecutor;
use a2a_protocol_sdk::server::request_context::RequestContext;
use a2a_protocol_sdk::server::streaming::EventQueueWriter;
use a2a_protocol_sdk::server::tool_loop::ANSWER_ARTIFACT_ID;
use a2a_protocol_sdk::types::budget::Budget;
use a2a_protocol_sdk::types::error::{A2aError, A2aResult};
use a2a_protocol_sdk::types::{Message, MessageSendParams, Part, TaskQueryParams, TaskState};

use crate::tools::log;

pub struct Coordinator {
    pub worker: Arc<A2aClient>,
    pub ask: u64,
    pub ask_time: Option<Duration>,
}

type Fut<'a> = Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

impl AgentExecutor for Coordinator {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            let budget = ctx
                .budget()
                .ok_or_else(|| A2aError::invalid_params("send a budget to delegate with"))?;
            let mut request = Budget::new()
                .with_meter("tokens", self.ask)
                .map_err(|e| A2aError::internal(e.to_string()))?;
            if let Some(t) = self.ask_time {
                request = request.with_time(t);
            }
            let grant = budget
                .grant(&request)
                .map_err(|e| A2aError::internal(e.to_string()))?;
            log(&format!("DELEGATION grant={:?}", grant.budget()));
            let text = ctx.message.text().unwrap_or_default().to_owned();
            let params =
                MessageSendParams::new(Message::user_text(uuid::Uuid::new_v4().to_string(), text));
            let started = std::time::Instant::now();
            let child = Delegation::start_budgeted(Arc::clone(&self.worker), params, grant, None)
                .await
                .map_err(|e| {
                    log(&format!("DELEGATION start failed: {e}"));
                    A2aError::internal(e.to_string())
                })?;
            let done = child.wait(ctx.cancellation_token.cancelled()).await;
            log(&format!(
                "DELEGATION ended after {:.1}s task={:?} outcome={}",
                started.elapsed().as_secs_f64(),
                done.task_id,
                match &done.outcome {
                    Outcome::Completed(_) => "Completed".to_owned(),
                    Outcome::Failed { class, status } => format!(
                        "Failed class={class:?} state={} message={:?}",
                        status.state,
                        status.message.as_ref().and_then(Message::text)
                    ),
                    other => format!("{other:?}"),
                }
            ));
            if let Outcome::Failed { status, .. } | Outcome::Canceled(status) = &done.outcome {
                let why = status.message.as_ref().and_then(Message::text).unwrap_or_default();
                return Err(A2aError::internal(format!(
                    "the worker's task ended {}: {why}",
                    status.state
                )));
            }
            if !matches!(done.outcome, Outcome::Completed(_)) {
                return Err(A2aError::internal(format!(
                    "the worker did not complete: {:?}",
                    done.outcome
                )));
            }
            let id = done
                .task_id
                .ok_or_else(|| A2aError::internal("the worker's task was never named"))?;
            let task = self
                .worker
                .get_task(TaskQueryParams::new(id.clone()))
                .await
                .map_err(|e| A2aError::internal(e.to_string()))?;
            let answer = task
                .artifacts
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|a| a.id.0 == ANSWER_ARTIFACT_ID)
                .and_then(|a| a.parts.iter().find_map(Part::text_content))
                .unwrap_or_default()
                .to_owned();
            emit.artifact(
                "relay",
                vec![Part::text(format!("worker task {id}: {answer}"))],
                None,
                Some(true),
            )
            .await?;
            emit.status(TaskState::Completed).await
        })
    }
}
