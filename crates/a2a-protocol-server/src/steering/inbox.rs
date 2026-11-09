// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The inbox of a running turn: where steering messages wait for the executor
//! to read them (ADR 0036).

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

use a2a_protocol_types::message::Message;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

/// A steering message as the executor receives it: the message, and who sent
/// it as authentication established.
///
/// The text has no more authority than its sender has over the task, which is
/// the authority of any other message sent to it
/// ([`docs/injection-containment.md`]): it does not cancel, and it is not an
/// approval. Treat it as the user text it is.
///
/// [`docs/injection-containment.md`]: https://github.com/tomtom215/a2a-rust/blob/main/docs/injection-containment.md
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Steered {
    /// The message, as sent and validated.
    pub message: Message,
    /// The caller's authenticated identity
    /// ([`CallContext::caller_identity`](crate::CallContext::caller_identity)),
    /// when authentication established one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

impl Steered {
    /// A steering message from `from`.
    #[must_use]
    pub const fn new(message: Message, from: Option<String>) -> Self {
        Self { message, from }
    }
}

/// Why [`Inbox::try_push`] did not take a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refused {
    /// The inbox already holds as many messages as it may.
    Full,
    /// The turn the inbox belongs to has ended.
    Closed,
}

/// The messages sent to a running turn, oldest first, bounded.
///
/// The server holds one per turn when steering is on
/// ([`RequestContext::inbox`](crate::RequestContext::inbox)) and closes it
/// when the executor returns. An executor that never reads it is unaffected:
/// what is in it is dropped with the turn, and counted as `unread`.
///
/// Built directly ([`new`](Self::new)) for an executor driven without a
/// server, as in a test; pass it to the context with
/// [`RequestContext::with_inbox`](crate::RequestContext::with_inbox).
///
/// ```rust
/// use a2a_protocol_server::{Inbox, Steered};
/// use a2a_protocol_types::message::Message;
///
/// let inbox = Inbox::new(2);
/// let steered = Steered::new(Message::user_text("m-1", "use the cheaper index"), None);
/// inbox.try_push(steered.clone()).unwrap();
/// assert_eq!(inbox.try_recv(), Some(steered));
/// assert_eq!(inbox.try_recv(), None);
/// ```
#[derive(Debug)]
pub struct Inbox {
    capacity: usize,
    state: Mutex<State>,
    ready: Notify,
}

#[derive(Debug, Default)]
struct State {
    queue: VecDeque<Steered>,
    closed: bool,
}

impl Inbox {
    /// An empty inbox that holds at most `capacity` messages.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(State::default()),
            ready: Notify::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds `steered` at the end, waking a reader waiting in
    /// [`recv`](Self::recv). Never waits.
    ///
    /// # Errors
    ///
    /// [`Refused::Closed`] once the turn has ended, [`Refused::Full`] when the
    /// inbox holds its capacity. Nothing is added then.
    pub fn try_push(&self, steered: Steered) -> Result<(), Refused> {
        let mut state = self.lock();
        if state.closed {
            return Err(Refused::Closed);
        }
        if state.queue.len() >= self.capacity {
            return Err(Refused::Full);
        }
        state.queue.push_back(steered);
        drop(state);
        self.ready.notify_one();
        Ok(())
    }

    /// The oldest message, if there is one. Never waits.
    #[must_use]
    pub fn try_recv(&self) -> Option<Steered> {
        self.lock().queue.pop_front()
    }

    /// The oldest message, waiting for one to arrive; `None` once the turn
    /// has ended and nothing is left. Safe to cancel: a message is taken only
    /// when this returns it.
    pub async fn recv(&self) -> Option<Steered> {
        loop {
            {
                let mut state = self.lock();
                if let Some(steered) = state.queue.pop_front() {
                    return Some(steered);
                }
                if state.closed {
                    return None;
                }
            }
            // A permit stored by a push or a close that came after the check
            // above is taken here, so nothing is missed between the two.
            self.ready.notified().await;
        }
    }

    /// How many messages wait.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().queue.len()
    }

    /// Whether no message waits.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().queue.is_empty()
    }

    /// Ends the inbox: later pushes are refused as [`Refused::Closed`], and a
    /// reader waiting in [`recv`](Self::recv) is woken. What is already in it
    /// can still be read. Returns how many messages were left unread.
    pub fn close(&self) -> usize {
        let mut state = self.lock();
        state.closed = true;
        let unread = state.queue.len();
        drop(state);
        self.ready.notify_waiters();
        self.ready.notify_one();
        unread
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    fn steered(id: &str) -> Steered {
        Steered::new(Message::user_text(id, id), Some("alice".to_owned()))
    }

    fn id(steered: &Steered) -> String {
        steered.message.id.0.clone()
    }

    #[test]
    fn messages_come_out_in_the_order_they_went_in() {
        let inbox = Inbox::new(3);
        for m in ["a", "b", "c"] {
            inbox.try_push(steered(m)).expect("room");
        }
        let order: Vec<String> = std::iter::from_fn(|| inbox.try_recv())
            .map(|s| id(&s))
            .collect();
        assert_eq!(order, ["a", "b", "c"]);
        assert_eq!(inbox.try_recv(), None);
    }

    /// One below, at and past the capacity, with the exact refusal each time.
    #[test]
    fn the_inbox_takes_its_capacity_and_not_one_more() {
        let inbox = Inbox::new(2);
        assert_eq!(inbox.try_push(steered("a")), Ok(()));
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox.try_push(steered("b")), Ok(()));
        assert_eq!(inbox.len(), 2);
        assert_eq!(inbox.try_push(steered("c")), Err(Refused::Full));
        assert_eq!(inbox.len(), 2, "a refused message is not kept");
        // Reading one makes room for exactly one.
        assert_eq!(inbox.try_recv().map(|s| id(&s)).as_deref(), Some("a"));
        assert_eq!(inbox.try_push(steered("d")), Ok(()));
        assert_eq!(inbox.try_push(steered("e")), Err(Refused::Full));
    }

    #[test]
    fn an_inbox_of_nothing_refuses_everything() {
        let inbox = Inbox::new(0);
        assert_eq!(inbox.try_push(steered("a")), Err(Refused::Full));
        assert!(inbox.is_empty());
    }

    #[test]
    fn len_and_is_empty_say_how_many_wait() {
        let inbox = Inbox::new(5);
        assert_eq!((inbox.len(), inbox.is_empty()), (0, true));
        inbox.try_push(steered("a")).expect("room");
        assert_eq!((inbox.len(), inbox.is_empty()), (1, false));
        inbox.try_push(steered("b")).expect("room");
        inbox.try_push(steered("c")).expect("room");
        assert_eq!((inbox.len(), inbox.is_empty()), (3, false));
    }

    #[test]
    fn closing_refuses_later_messages_and_reports_the_unread() {
        let inbox = Inbox::new(5);
        inbox.try_push(steered("a")).expect("room");
        inbox.try_push(steered("b")).expect("room");
        assert_eq!(inbox.close(), 2);
        assert_eq!(inbox.try_push(steered("c")), Err(Refused::Closed));
        assert_eq!(inbox.len(), 2, "a refused message is not kept");
        // What was there can still be read.
        assert_eq!(inbox.try_recv().map(|s| id(&s)).as_deref(), Some("a"));
        assert_eq!(inbox.close(), 1, "closing again reports what is left");
    }

    /// A closed inbox that is also full says it is closed.
    #[test]
    fn closed_is_reported_before_full() {
        let inbox = Inbox::new(1);
        inbox.try_push(steered("a")).expect("room");
        inbox.close();
        assert_eq!(inbox.try_push(steered("b")), Err(Refused::Closed));
    }

    #[tokio::test(start_paused = true)]
    async fn recv_waits_for_a_message_and_takes_it() {
        let inbox = Arc::new(Inbox::new(2));
        let mut reader = {
            let inbox = Arc::clone(&inbox);
            tokio::spawn(async move { inbox.recv().await })
        };
        // Nothing is there yet: with the clock paused, a second passes at
        // once and the reader is still waiting.
        let waiting = tokio::time::timeout(Duration::from_secs(1), &mut reader).await;
        assert!(waiting.is_err(), "recv returned with nothing to return");
        inbox.try_push(steered("late")).expect("room");
        let got = reader.await.expect("joins");
        assert_eq!(got.map(|s| id(&s)).as_deref(), Some("late"));
        assert!(inbox.is_empty(), "the message was taken, not copied");
    }

    #[tokio::test]
    async fn recv_returns_what_is_already_there_without_waiting() {
        let inbox = Inbox::new(2);
        inbox.try_push(steered("a")).expect("room");
        assert_eq!(inbox.recv().await.map(|s| id(&s)).as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn close_wakes_a_waiting_reader_with_none() {
        let inbox = Arc::new(Inbox::new(2));
        let reader = {
            let inbox = Arc::clone(&inbox);
            tokio::spawn(async move { inbox.recv().await })
        };
        // The reader may or may not be waiting yet; the answer is the same.
        assert_eq!(inbox.close(), 0);
        let got = tokio::time::timeout(Duration::from_secs(10), reader)
            .await
            .expect("a closed inbox does not leave a reader waiting")
            .expect("joins");
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn a_closed_inbox_hands_out_what_is_left_then_none() {
        let inbox = Inbox::new(2);
        inbox.try_push(steered("a")).expect("room");
        inbox.close();
        assert_eq!(inbox.recv().await.map(|s| id(&s)).as_deref(), Some("a"));
        assert_eq!(inbox.recv().await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_recv_that_is_dropped_takes_nothing() {
        let inbox = Inbox::new(2);
        let timed_out = tokio::time::timeout(Duration::from_secs(1), inbox.recv()).await;
        assert!(timed_out.is_err());
        inbox.try_push(steered("a")).expect("room");
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox.recv().await.map(|s| id(&s)).as_deref(), Some("a"));
    }

    #[test]
    fn a_message_serializes_with_its_sender_and_reads_back() {
        let with = steered("a");
        let wire = serde_json::to_value(&with).expect("serializes");
        assert_eq!(wire["from"], "alice");
        assert_eq!(wire["message"]["messageId"], "a");
        assert_eq!(
            serde_json::from_value::<Steered>(wire).expect("reads"),
            with
        );

        let anonymous = Steered::new(Message::user_text("b", "b"), None);
        let wire = serde_json::to_value(&anonymous).expect("serializes");
        assert!(wire.get("from").is_none(), "no sender, no key: {wire}");
        assert_eq!(
            serde_json::from_value::<Steered>(wire).expect("reads"),
            anonymous
        );
    }
}
