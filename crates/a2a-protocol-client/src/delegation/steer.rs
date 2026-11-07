// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Steering a delegated child while it works (ADR 0036): a message to the
//! child's running task, delivered to its executor's inbox.
//!
//! [`Delegation::steer`] sends one, and [`steer_all`] sends one to each of
//! several children at once, reporting what became of each. The agent must
//! take steering (its card declares
//! `https://a2a-rust.com/extensions/steering/v1`); one that does not refuses
//! the message as it refuses any message to a working task, and that refusal
//! is the error you get. The reply says where the message went and never that
//! the executor acted on it ([`Steering`]).

use std::sync::Arc;

use a2a_protocol_types::error::A2aError;
use a2a_protocol_types::idempotency;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::steering::{self, Delivery, Receipt};
use a2a_protocol_types::task::{ContextId, Task, TaskId};
use a2a_protocol_types::{Message, MessageSendParams};

use super::Delegation;
use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};

/// What the agent did with a steering message.
#[derive(Debug)]
#[non_exhaustive]
pub enum Steering {
    /// The message is in the running turn's inbox, or on its way to the
    /// replica that holds the turn, or was already delivered by an earlier
    /// attempt: [`Delivery`] says which. It does not say the executor read or
    /// acted on it.
    Delivered {
        /// Where the message went.
        delivery: Delivery,
        /// The task as the agent reported it.
        task: Task,
    },
    /// The agent answered, but did not deliver the message to a running
    /// turn: the task was not running (it had parked at `input-required`, say)
    /// and the message was handled as any other message to it. What the agent
    /// answered is here; look at the task's state to see what happened.
    NotSteered(SendMessageResponse),
}

/// Steers one child, from anywhere.
///
/// It owns what it needs and borrows nothing from the [`Delegation`], so a
/// parent can steer a child from one task while another waits on it. From
/// [`Delegation::steer_handle`].
#[derive(Debug, Clone)]
pub struct SteerHandle {
    client: Arc<A2aClient>,
    tenant: Option<String>,
    task_id: String,
    context_id: Option<String>,
}

impl SteerHandle {
    /// The child's task id.
    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Sends `message` to the child's running task: names the task (and its
    /// context, when known), lists the steering extension, and, when the
    /// agent is known to honour idempotency keys and the message has none,
    /// gives it a fresh one, so a client with a retry policy can retry a
    /// delivery whose answer was lost without delivering it twice.
    ///
    /// # Errors
    ///
    /// The client's: the agent refused the message (a full inbox is `503`
    /// with `Retry-After`; a task that ended is an error naming it; an agent
    /// that does not take steering refuses it as it refuses any message to a
    /// working task), or the call failed. An answer that carries a receipt
    /// that is not one is [`ClientError::Protocol`].
    pub async fn steer(&self, mut message: Message) -> ClientResult<Steering> {
        message.task_id = Some(TaskId::new(self.task_id.as_str()));
        if let Some(context) = &self.context_id {
            message.context_id = Some(ContextId::new(context.as_str()));
        }
        steering::declare(&mut message);
        if self.client.honours_idempotency() && matches!(idempotency::key_of(&message), Ok(None)) {
            // A fresh key is 64 hex digits, always valid, so this cannot fail.
            let _ = idempotency::set_key(&mut message, &super::fresh_key());
        }
        let mut params = MessageSendParams::new(message);
        params.tenant.clone_from(&self.tenant);
        let response = self.client.send_message(params).await?;
        let receipt = match &response {
            SendMessageResponse::Task(task) => {
                Receipt::read(task).map_err(ClientError::Protocol)?
            }
            _ => None,
        };
        Ok(match (receipt, response) {
            (Some(receipt), SendMessageResponse::Task(task)) => Steering::Delivered {
                delivery: receipt.delivery,
                task,
            },
            (_, response) => Steering::NotSteered(response),
        })
    }
}

impl Delegation {
    /// A handle that steers this child from anywhere ([`SteerHandle`]), once
    /// an event has named the child; `None` before that.
    #[must_use]
    pub fn steer_handle(&self) -> Option<SteerHandle> {
        Some(SteerHandle {
            client: Arc::clone(&self.client),
            tenant: self.tenant.clone(),
            task_id: self.task_id.clone()?,
            context_id: self.context_id.clone(),
        })
    }

    /// Sends `message` to the child's running task, so its executor can
    /// read it between steps; see [`SteerHandle::steer`] for what is added to
    /// the message and how it can be refused.
    ///
    /// The child must have been named by an event
    /// ([`next_event`](Self::next_event), or [`task_id`](Self::task_id)
    /// answering `Some`): nothing is sent before that.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] when the child has not been named yet;
    /// [`SteerHandle::steer`]'s otherwise.
    pub async fn steer(&self, message: Message) -> ClientResult<Steering> {
        match self.steer_handle() {
            Some(handle) => handle.steer(message).await,
            None => Err(not_named()),
        }
    }
}

fn not_named() -> ClientError {
    ClientError::Transport(
        "the delegated child has not been named by an event yet, so there is no task to steer; \
         nothing was sent"
            .to_owned(),
    )
}

/// What became of the message sent to one child by [`steer_all`].
#[derive(Debug)]
#[non_exhaustive]
pub struct ChildSteering {
    /// The child's task id, when it had been named.
    pub task_id: Option<String>,
    /// What the agent did with the message, or why it was not delivered.
    pub result: ClientResult<Steering>,
}

/// One child's steering send, running: its task id, and the send if it could be made.
type Sent = (
    Option<String>,
    Option<tokio::task::JoinHandle<ClientResult<Steering>>>,
);

/// Steers each of `children` at once, with a message made by `message` for
/// each (a message needs its own id, so the closure is called once per
/// child), and returns what became of each, in the order given.
///
/// Waits for every child: each send is bounded by the client's own timeout,
/// one that fails or is refused does not stop the others, and the caller
/// learns the whole outcome at once, which is what a fan-out of corrections
/// is for. A child not yet named is reported as such, with nothing sent.
/// ADR 0033's group of delegations will wrap this.
///
/// ```rust,no_run
/// use a2a_protocol_client::delegation::{Delegation, steer_all};
/// use a2a_protocol_types::Message;
/// # async fn run(children: Vec<Delegation>) {
/// let outcomes = steer_all(&children, || {
///     Message::user_text(uuid::Uuid::new_v4().to_string(), "stop at the first match")
/// })
/// .await;
/// for child in outcomes {
///     println!("{:?}: {:?}", child.task_id, child.result.is_ok());
/// }
/// # }
/// ```
pub async fn steer_all<'a, I, F>(children: I, mut message: F) -> Vec<ChildSteering>
where
    I: IntoIterator<Item = &'a Delegation>,
    F: FnMut() -> Message,
{
    let sent: Vec<Sent> = children
        .into_iter()
        .map(|child| {
            let task_id = child.task_id().map(str::to_owned);
            let handle = child.steer_handle().map(|handle| {
                let message = message();
                tokio::spawn(async move { handle.steer(message).await })
            });
            (task_id, handle)
        })
        .collect();
    let mut outcomes = Vec::with_capacity(sent.len());
    for (task_id, handle) in sent {
        let result = match handle {
            None => Err(not_named()),
            Some(handle) => handle.await.unwrap_or_else(|e| {
                Err(ClientError::Protocol(A2aError::internal(format!(
                    "the steering send panicked: {e}"
                ))))
            }),
        };
        outcomes.push(ChildSteering { task_id, result });
    }
    outcomes
}
