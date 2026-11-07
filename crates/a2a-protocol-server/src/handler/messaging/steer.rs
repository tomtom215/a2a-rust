// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A message to a running task: put in its turn's inbox instead of refused
//! (ADR 0036, [`crate::steering`]).
//!
//! The decision is taken after the message has been validated and before the
//! tenant's slot, the admission gate and the per-context lock: a steering
//! message starts no turn, so it must neither wait for a slot a running turn
//! holds nor be refused because the tenant is at its cap. What is left of a
//! send that is not steering is exactly the send it would have been: this
//! module returns `None` for it, having changed nothing.

use std::sync::Arc;

use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::steering::{Delivery, Receipt};
use a2a_protocol_types::task::{Task, TaskId, TaskState};

use super::super::{CancellationEntry, ExecutorTurn, RequestHandler};
use super::decisions::second_send_blocked;
use super::idempotency::SendKey;
use crate::call_context::CallContext;
use crate::error::{ServerError, ServerResult};
use crate::metrics::control::steering as outcome;
use crate::steering::{Refused, Steered, SteeringConfig};
use crate::store::claim::Handed;
use crate::store::tenant::TenantContext;

/// Whether the task's stored state is one a running turn leaves it in: still
/// `working`, or `submitted` in the moment before the executor's first write.
/// A parked task (`input-required`, `auth-required`) or a finished one is not
/// running, whatever else is true of it.
pub(super) const fn running_state(state: TaskState) -> bool {
    matches!(state, TaskState::Working | TaskState::Submitted)
}

/// Whether the turn in `entry` can be steered: it is this tenant's, its token
/// is live, and it has not parked the task.
pub(super) fn steerable(entry: &CancellationEntry, tenant: &str) -> bool {
    entry.tenant == tenant && second_send_blocked(entry) && !entry.turn.is_parked()
}

/// What the turn's replica did with a message.
enum Placed {
    /// In the inbox of the turn running here.
    Inbox,
    /// In the store, for the replica that holds the turn's claim.
    HandedOff,
}

impl RequestHandler {
    /// Whether `message` asks to be steered, and this handler takes steering:
    /// it names a task and lists the extension. Whether it *is* steering
    /// depends on the task, which [`Self::steer_if_running`] reads.
    pub(super) fn asks_to_steer(&self, message: &Message) -> bool {
        self.steering.is_some()
            && message.task_id.is_some()
            && a2a_protocol_types::steering::declares_extension(message)
    }

    /// Delivers `params.message` to the turn running its task, if one does:
    /// the task as it stands, carrying the receipt; `None` when the task is
    /// not running (absent, parked, finished, in another context, or running
    /// nowhere this handler can reach), in which case nothing was changed and
    /// the send goes on as any other.
    ///
    /// # Errors
    ///
    /// Only for a task that is running: [`ServerError::Halted`] for a halted
    /// job, [`ServerError::PayloadTooLarge`] for a message over the
    /// configured size, [`ServerError::Overloaded`] when the inbox is full or
    /// the turn ended first (nothing was delivered, and the idempotency key,
    /// if any, is free again), the key's own refusals, and the store's.
    pub(super) async fn steer_if_running(
        &self,
        params: &MessageSendParams,
        call: &CallContext,
    ) -> ServerResult<Option<Task>> {
        let (Some(config), Some(task_id)) = (self.steering, params.message.task_id.as_ref()) else {
            return Ok(None);
        };
        let message = &params.message;
        let Some((mut task, local)) = self.running_task(message, task_id).await? else {
            return Ok(None);
        };

        // The checks a send makes that apply to a message that starts no
        // turn: the job's halt, and the job it names being the task's own.
        let (lineage, _) =
            super::super::lineage::of_turn(message, Some(&task), task.context_id.0.as_str())?;
        if let Err(halted) = self.refuse_if_halted(&lineage.root).await {
            self.metrics.on_steering(outcome::HALTED);
            return Err(halted);
        }
        self.check_size(message, &config)?;

        let key = match self.claim_send_key(message, task_id).await? {
            SendKey::Absent => None,
            SendKey::Claimed(key) => Some(key),
            SendKey::Replay(earlier) => return self.replayed(*earlier, task_id),
        };
        let from = call.caller_identity().map(str::to_owned);
        let placed = match self.place(task_id, local, message, from, &config).await {
            Ok(placed) => placed,
            Err(refusal) => {
                if let Some(key) = key {
                    self.release_send_key(&key).await;
                }
                return Err(refusal);
            }
        };
        let delivery = match placed {
            Placed::Inbox => Delivery::Inbox,
            Placed::HandedOff => Delivery::HandedOff,
        };
        self.metrics.on_steering(match placed {
            Placed::Inbox => outcome::DELIVERED,
            Placed::HandedOff => outcome::HANDED_OFF,
        });
        trace_info!(
            task_id = %task_id,
            outcome = delivery.as_str(),
            "steering message accepted"
        );
        #[cfg(feature = "audit")]
        if let Some(log) = &self.audit {
            crate::audit::record_steered(log, &*self.metrics, call, &task, message, delivery).await;
        }
        Receipt::new(delivery).attach(&mut task);
        Ok(Some(task))
    }

    /// The task `message` names, and the turn running it on this handler if
    /// there is one, when the task is running somewhere a message can be
    /// delivered; `None` when it is not (see [`Self::steer_if_running`]).
    async fn running_task(
        &self,
        message: &Message,
        task_id: &TaskId,
    ) -> ServerResult<Option<(Task, Option<Arc<ExecutorTurn>>)>> {
        // Read in the caller's partition: another tenant's or caller's task
        // is not found here, as it is not found by any other operation.
        let Some(task) = self.task_store.get(task_id).await? else {
            return Ok(None);
        };
        if message
            .context_id
            .as_ref()
            .is_some_and(|c| *c != task.context_id)
            || !running_state(task.status.state)
        {
            return Ok(None);
        }
        let local = self.running_turn(task_id, &TenantContext::current()).await;
        // With no turn here and no claims, nothing can be running anywhere
        // this handler could hand a message to.
        if local.is_none() && self.turn_claims.is_none() {
            return Ok(None);
        }
        Ok(Some((task, local)))
    }

    /// The turn of `task_id` running on this handler that a message can be
    /// delivered to.
    async fn running_turn(&self, task_id: &TaskId, tenant: &str) -> Option<Arc<ExecutorTurn>> {
        self.cancellation_tokens
            .read()
            .await
            .get(task_id)
            .filter(|entry| steerable(entry, tenant))
            .map(|entry| Arc::clone(&entry.turn))
    }

    /// Refuses a message over the configured size.
    fn check_size(&self, message: &Message, config: &SteeringConfig) -> ServerResult<()> {
        let size = serde_json::to_vec(message)?.len();
        if size > config.max_message_size() {
            self.metrics.on_steering(outcome::TOO_LARGE);
            return Err(ServerError::PayloadTooLarge(format!(
                "a steering message is {size} bytes, over this server's limit of {} bytes",
                config.max_message_size()
            )));
        }
        Ok(())
    }

    /// The answer to a retry of a steering message that an earlier attempt
    /// delivered: the task, with a receipt saying nothing was delivered again.
    fn replayed(&self, earlier: Task, task_id: &TaskId) -> ServerResult<Option<Task>> {
        // A key is the handle to the task its first send named. One that
        // names another task is a key reused across two sends, never a retry.
        if earlier.id != *task_id {
            return Err(ServerError::InvalidParams(format!(
                "the idempotency key was used for a message to task {}, not this one",
                earlier.id
            )));
        }
        self.metrics.on_steering(outcome::REPLAYED);
        let mut task = earlier;
        Receipt::new(Delivery::Replayed).attach(&mut task);
        Ok(Some(task))
    }

    /// Puts the message where the turn reads it: its inbox when the turn runs
    /// here, the store when another replica holds its claim.
    async fn place(
        &self,
        task_id: &TaskId,
        local: Option<Arc<ExecutorTurn>>,
        message: &Message,
        from: Option<String>,
        config: &SteeringConfig,
    ) -> ServerResult<Placed> {
        let steered = Steered::new(message.clone(), from);
        if let Some(inbox) = local.as_ref().and_then(|turn| turn.inbox()) {
            return match inbox.try_push(steered) {
                Ok(()) => Ok(Placed::Inbox),
                Err(Refused::Full) => Err(self.full(task_id, config)),
                Err(Refused::Closed) => Err(self.ended(task_id)),
            };
        }
        // A turn running here whose handler has no inbox for it cannot be
        // reached: it started before steering was on, which `build` makes
        // impossible, so this is another replica's turn by elimination.
        match self.hand_steering(task_id, &steered, config).await? {
            Handed::Queued => Ok(Placed::HandedOff),
            Handed::Full => Err(self.full(task_id, config)),
            Handed::NoClaim => Err(self.ended(task_id)),
            Handed::Unsupported => Err(ServerError::UnsupportedOperation(format!(
                "task {task_id} is being processed by another replica, and the turn claim \
                 store this server uses cannot hand it a steering message"
            ))),
        }
    }

    /// The refusal of a message that finds the inbox full.
    fn full(&self, task_id: &TaskId, config: &SteeringConfig) -> ServerError {
        self.metrics.on_steering(outcome::FULL);
        ServerError::Overloaded(format!(
            "the steering inbox of task {task_id} holds its {} messages; retry later",
            config.inbox_capacity()
        ))
    }

    /// The refusal of a message whose turn ended before it was delivered.
    fn ended(&self, task_id: &TaskId) -> ServerError {
        self.metrics.on_steering(outcome::ENDED);
        ServerError::Overloaded(format!(
            "the turn of task {task_id} ended before the steering message was delivered; \
             nothing was delivered, retry to see what the task became"
        ))
    }
}
