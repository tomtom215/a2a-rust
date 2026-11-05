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
use super::{DEFAULT_ID_WAIT, Delegation, Outcome, ResumePolicy};
use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;

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
            opened_at: None,
            events_since_open: 0,
            message: None,
            context_id: None,
            handed_back: false,
            continued: 0,
            hand_off_policy: super::HandOffPolicy::default(),
            journal: None,
        };
        match this.reopen(&id).await {
            Ok(stream) => {
                this.stream = Some(stream);
                this.stream_opened();
            }
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

    /// Notes that a stream has just been opened: the clock a later loss is
    /// judged by starts here, with no event seen on it yet.
    pub(super) fn stream_opened(&mut self) {
        self.opened_at = Some(tokio::time::Instant::now());
        self.events_since_open = 0;
    }

    /// Whether the stream that was lost delivered at least one event and had
    /// been open for `healthy_after`.
    fn stream_worked(&self, healthy_after: Duration) -> bool {
        self.events_since_open > 0
            && self
                .opened_at
                .is_some_and(|at| at.elapsed() >= healthy_after)
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
        let mut query = TaskQueryParams::new(id).with_history_length(0);
        query.tenant.clone_from(&self.tenant);
        let task = self.client.get_task(query).await.ok()?;
        Outcome::of(task.status)
    }

    /// Works on a lost stream until it is open again, the child is found
    /// settled, or the policy is spent. See [`ResumePolicy`].
    pub(super) async fn resume(&mut self) -> Resumed {
        let Some(id) = self.task_id.clone() else {
            return Resumed::GaveUp;
        };
        let policy = self.resume_policy;
        if self.last_event_id != self.resumed_at || self.stream_worked(policy.healthy_after) {
            // The position moved since the last loss, or the stream stayed up
            // and delivered: that stream worked.
            self.streak = 0;
        }
        let started = tokio::time::Instant::now();
        loop {
            if self.streak >= policy.attempts {
                break;
            }
            let wait = policy.attempt_wait(self.streak, crate::retry::jittered);
            if !wait.is_zero() {
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
                    self.stream_opened();
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

#[cfg(test)]
mod tests {
    use super::super::testing::parked;
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_has_worked_only_with_an_event_and_once_it_was_open_long_enough() {
        let five = Duration::from_secs(5);
        let mut d = parked();
        assert!(!d.stream_worked(five), "never opened");
        d.stream_opened();
        tokio::time::advance(Duration::from_secs(600)).await;
        assert!(!d.stream_worked(five), "open for ten minutes with no event");

        d.stream_opened();
        d.events_since_open = 1;
        tokio::time::advance(ms(4_999)).await;
        assert!(!d.stream_worked(five), "one millisecond short");
        tokio::time::advance(ms(1)).await;
        assert!(d.stream_worked(five), "exactly the healthy time counts");
        assert!(d.stream_worked(Duration::ZERO));
    }

    #[tokio::test(start_paused = true)]
    async fn opening_a_stream_forgets_the_events_of_the_one_before() {
        let mut d = parked();
        d.stream_opened();
        d.events_since_open = 9;
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(d.stream_worked(Duration::from_secs(5)));
        d.stream_opened();
        assert_eq!(d.events_since_open, 0);
        assert!(
            !d.stream_worked(Duration::ZERO),
            "a new stream has no events"
        );
    }

    /// A listener that counts connections and closes each at once.
    async fn counting_listener() -> (
        std::net::SocketAddr,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let count = std::sync::Arc::new(AtomicUsize::new(0));
        let seen = std::sync::Arc::clone(&count);
        tokio::spawn(async move {
            while let Ok((_socket, _)) = listener.accept().await {
                seen.fetch_add(1, Ordering::SeqCst);
            }
        });
        (addr, count)
    }

    async fn resume_against(
        addr: std::net::SocketAddr,
        worked: bool,
    ) -> (usize, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let client = std::sync::Arc::new(
            crate::ClientBuilder::new(format!("http://{addr}"))
                .build()
                .expect("a client"),
        );
        let mut d = parked();
        d.client = client;
        d.resume_policy = ResumePolicy::default()
            .with_attempts(2)
            .with_backoff(Duration::ZERO, Duration::ZERO)
            .with_jitter(false)
            .with_healthy_after(Duration::ZERO);
        // The count is spent: without a reason to start it again nothing is tried.
        d.streak = 2;
        d.stream_opened();
        d.events_since_open = u32::from(worked);
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _ = &count;
        let outcome = d.resume().await;
        assert!(matches!(outcome, Resumed::GaveUp), "nothing answers");
        (d.streak as usize, count)
    }

    #[tokio::test]
    async fn a_stream_that_worked_gets_its_attempts_back_and_one_that_did_not_does_not() {
        use std::sync::atomic::Ordering;
        let (addr, hits) = counting_listener().await;
        let (streak, _) = resume_against(addr, false).await;
        assert_eq!(
            streak, 2,
            "a spent count with no healthy stream tries nothing"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let (streak, _) = resume_against(addr, true).await;
        assert_eq!(streak, 2, "the two attempts were made again");
        assert!(
            hits.load(Ordering::SeqCst) >= 2,
            "the agent was asked again after a stream that worked"
        );
    }
}
