// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The state of one executor turn, as the executor's writer, its cleanup,
//! admission and the claims' renewer share it.

use std::sync::Arc;

/// The state of one executor turn, shared by the executor's writer, its
/// cleanup, and admission.
#[derive(Debug, Default)]
pub struct ExecutorTurn {
    /// The last state the executor wrote was `input-required` or
    /// `auth-required`. Set *before* the event reaches the queue, so no
    /// reader can see the interrupted state while this still says otherwise.
    pub(crate) parked: std::sync::atomic::AtomicBool,
    /// Cancelled once the executor's queue and token have been released —
    /// the moment a continuation can be admitted.
    pub(crate) finished: tokio_util::sync::CancellationToken,
    /// A [`halt`](RequestHandler::halt) stopped this turn. Set before the
    /// task's token is cancelled, so an executor that returns on the token
    /// sees it, and its task is ended `Canceled` as on shutdown.
    pub(crate) halted: std::sync::atomic::AtomicBool,
    /// This replica could not keep the turn's claim (`handler::claims`): its
    /// renewals went unanswered for most of the claim's time to live, so
    /// another replica may take the task over. Set before the task's token is
    /// cancelled, so an executor that returns on the token sees it, and its
    /// task is ended `Failed`, failure class `transient`, never `Canceled`.
    pub(crate) lapsed: std::sync::atomic::AtomicBool,
    /// Where steering messages for this turn wait (ADR 0036): `Some` when
    /// the handler takes steering. Closed when the executor returns.
    inbox: Option<Arc<crate::steering::Inbox>>,
}

impl ExecutorTurn {
    /// A turn whose executor reads `inbox`, when steering is on.
    pub(crate) fn with_inbox(inbox: Option<Arc<crate::steering::Inbox>>) -> Self {
        Self {
            inbox,
            ..Self::default()
        }
    }

    /// The turn's inbox, when the handler takes steering.
    pub(crate) const fn inbox(&self) -> Option<&Arc<crate::steering::Inbox>> {
        self.inbox.as_ref()
    }

    /// Whether the executor's latest state asks the client for input.
    pub(crate) fn is_parked(&self) -> bool {
        self.parked.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Closes the inbox once the executor has returned, and counts what it
    /// left unread: later messages are refused as the turn ending, and what
    /// was never read is dropped with the turn, which is said in the log and
    /// counted (`unread`), never silent.
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub(crate) fn close_inbox(
        &self,
        metrics: &dyn crate::metrics::Metrics,
        task_id: &a2a_protocol_types::task::TaskId,
    ) {
        let Some(inbox) = &self.inbox else { return };
        let unread = inbox.close();
        for _ in 0..unread {
            metrics.on_steering(crate::metrics::control::steering::UNREAD);
        }
        if unread > 0 {
            trace_warn!(
                task_id = %task_id,
                unread,
                "the executor returned with steering messages it never read"
            );
        }
    }

    /// Marks the turn halted; the caller cancels its token next.
    pub(crate) fn halt(&self) {
        self.halted
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Whether a halt stopped this turn.
    pub(crate) fn is_halted(&self) -> bool {
        self.halted.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Marks the turn's claim lapsed; the caller cancels its token next.
    pub(crate) fn lapse(&self) {
        self.lapsed
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Whether this replica gave up the turn's claim.
    pub(crate) fn is_lapsed(&self) -> bool {
        self.lapsed.load(std::sync::atomic::Ordering::Acquire)
    }
}

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;
