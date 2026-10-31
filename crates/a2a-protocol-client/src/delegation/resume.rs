// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Getting a lost stream back, and picking a child up again (audit H4,
//! ADR 0030).
//!
//! Until 2026-10-31 any end of a child's stream before the child settled, a
//! reset connection, a proxy rollout or the idle timeout, was read as "the
//! child is lost": the delegation cancelled it. The client already had what a
//! resume needs (`EventStream::last_event_id`, `subscribe_to_task_from`, and
//! servers that replay from a `Last-Event-ID`) and nothing used it, so one
//! blip on one link killed healthy work, and in a tree of delegations the
//! cancel cascaded.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::TaskQueryParams;

use super::origin::Origin;
use super::{DEFAULT_ID_WAIT, Delegation, Outcome};
use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;

/// How a [`Delegation`] tries to get its stream back before it gives up on
/// the child. The default is five attempts, 200 ms apart at first and
/// doubling to 5 s, inside 30 s in all.
///
/// An attempt that gets a stream and then loses it without the position
/// moving counts as spent: a server that accepts a subscription and drops it
/// at once exhausts the policy rather than being asked for ever. A stream
/// that delivered events (the position advanced) starts the count again.
///
/// Positions come from the agent's event ids. An agent that sends none (gRPC
/// and WebSocket streams carry none) never shows progress, so every loss
/// costs an attempt and the policy bounds the whole delegation.
///
/// [`ResumePolicy::none`] restores the behaviour before 2026-10-31: the first
/// loss cancels the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResumePolicy {
    /// How many times in a row to reopen the stream. Zero never resumes.
    pub attempts: u32,
    /// The wait before the second attempt; each later wait doubles.
    pub backoff: Duration,
    /// The longest a wait between attempts grows to.
    pub max_backoff: Duration,
    /// The longest one loss is worked on, from the loss to giving up.
    pub deadline: Duration,
}

impl Default for ResumePolicy {
    fn default() -> Self {
        Self {
            attempts: 5,
            backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(5),
            deadline: Duration::from_secs(30),
        }
    }
}

impl ResumePolicy {
    /// Never resumes: the first lost stream cancels the child.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            attempts: 0,
            backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
            deadline: Duration::ZERO,
        }
    }

    /// Sets how many times in a row the stream is reopened.
    #[must_use]
    pub const fn with_attempts(mut self, attempts: u32) -> Self {
        self.attempts = attempts;
        self
    }

    /// Sets the first wait between attempts and the longest one.
    #[must_use]
    pub const fn with_backoff(mut self, first: Duration, longest: Duration) -> Self {
        self.backoff = first;
        self.max_backoff = longest;
        self
    }

    /// Sets how long one loss is worked on.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

/// What a lost stream came to.
pub(super) enum Resumed {
    /// The stream is open again; keep following.
    Streaming,
    /// The child had settled while the stream was down.
    Settled(Box<Outcome>),
    /// The policy is spent with the child still running or unreachable.
    GaveUp,
}

impl Delegation {
    /// Sets how a lost stream is resumed; see [`ResumePolicy`].
    #[must_use]
    pub const fn with_resume(mut self, policy: ResumePolicy) -> Self {
        self.resume_policy = policy;
        self
    }

    /// The position of the last event received: the id to resume from, to
    /// record with [`task_id`](Self::task_id) before [`detach`](Self::detach)
    /// so that [`reattach`](Self::reattach) can pick the child up again.
    /// `None` until an event carried one, and always `None` on gRPC and
    /// WebSocket streams.
    #[must_use]
    pub fn last_event_id(&self) -> Option<&str> {
        self.last_event_id.as_deref()
    }

    /// Follows a child that is already running, from the position
    /// `last_event_id` (or from its current state when `None`): a supervisor
    /// that recorded [`task_id`](Self::task_id) and
    /// [`last_event_id`](Self::last_event_id) before it stopped picks the
    /// child up after a restart.
    ///
    /// A child that has settled in the meantime returns a handle that is
    /// already settled, so [`wait`](Self::wait) answers at once with what it
    /// became. The handle owns the child again: dropped unsettled, it
    /// cancels. A lease or budget grant is not restored (ADR 0030).
    ///
    /// # Errors
    ///
    /// The subscription's error when the stream cannot be opened and the
    /// child's state cannot be read to show it settled. No handle exists
    /// then, and nothing is cancelled.
    pub async fn reattach(
        client: Arc<A2aClient>,
        tenant: Option<String>,
        task_id: impl Into<String>,
        last_event_id: Option<String>,
    ) -> ClientResult<Self> {
        let id = task_id.into();
        let mut this = Self {
            client,
            tenant,
            stream: None,
            task_id: Some(id.clone()),
            settled: None,
            armed: true,
            id_wait: DEFAULT_ID_WAIT,
            renewal: None,
            budget: None,
            idempotency_key: None,
            origin: Origin::current(),
            resume_policy: ResumePolicy::default(),
            last_event_id,
            streak: 0,
            resumed_at: None,
        };
        match this.reopen(&id).await {
            Ok(stream) => this.stream = Some(stream),
            Err(e) => {
                if let Some(outcome) = this.state_of(&id).await {
                    this.settled = Some(outcome);
                } else {
                    // Never held: dropping this must not cancel a child it
                    // could not even look at.
                    this.armed = false;
                    return Err(e);
                }
            }
        }
        Ok(this)
    }

    /// Records the position of the stream, from the stream itself or from the
    /// error that ended it.
    pub(super) fn remember_position(&mut self, error: Option<&ClientError>) {
        if let Some(ClientError::IncompleteStream {
            last_event_id: Some(id),
            ..
        }) = error
        {
            self.last_event_id = Some(id.clone());
        } else if let Some(id) = self.stream.as_ref().and_then(EventStream::last_event_id) {
            self.last_event_id = Some(id.to_owned());
        }
    }

    /// Opens the child's stream again from the recorded position.
    async fn reopen(&self, id: &str) -> ClientResult<EventStream> {
        self.client
            .subscribe_in(
                self.tenant.clone(),
                id.to_owned(),
                self.last_event_id.clone(),
            )
            .await
    }

    /// What the child has become, if it has settled: the A2A rule that a
    /// terminal task cannot be subscribed to makes this the usual reason a
    /// resubscription is refused.
    async fn state_of(&self, id: &str) -> Option<Outcome> {
        let task = self
            .client
            .get_task(TaskQueryParams {
                tenant: self.tenant.clone(),
                id: id.to_owned(),
                history_length: Some(0),
            })
            .await
            .ok()?;
        Outcome::of(task.status)
    }

    /// Works on a lost stream until it is open again, the child is found
    /// settled, or the policy is spent. See [`ResumePolicy`].
    pub(super) async fn resume(&mut self) -> Resumed {
        let Some(id) = self.task_id.clone() else {
            return Resumed::GaveUp;
        };
        let policy = self.resume_policy;
        if self.last_event_id != self.resumed_at {
            // The position moved since the last loss: that stream worked.
            self.streak = 0;
        }
        let started = tokio::time::Instant::now();
        loop {
            if self.streak >= policy.attempts {
                break;
            }
            if self.streak > 0 {
                let wait = policy
                    .backoff
                    .saturating_mul(1_u32 << (self.streak - 1).min(16))
                    .min(policy.max_backoff);
                let left = policy.deadline.saturating_sub(started.elapsed());
                if left.is_zero() {
                    break;
                }
                tokio::time::sleep(wait.min(left)).await;
            }
            let left = policy.deadline.saturating_sub(started.elapsed());
            if left.is_zero() {
                break;
            }
            self.streak += 1;
            match tokio::time::timeout(left, self.reopen(&id)).await {
                Ok(Ok(stream)) => {
                    trace_debug!(task_id = %id, "delegation: stream reopened");
                    self.stream = Some(stream);
                    self.resumed_at.clone_from(&self.last_event_id);
                    return Resumed::Streaming;
                }
                Ok(Err(_e)) => {
                    trace_debug!(task_id = %id, error = %_e, "delegation: stream not reopened");
                    // The child may have settled while the stream was down.
                    if let Some(outcome) = self.state_of(&id).await {
                        return Resumed::Settled(Box::new(outcome));
                    }
                }
                Err(_) => break,
            }
        }
        self.resumed_at.clone_from(&self.last_event_id);
        Resumed::GaveUp
    }
}
