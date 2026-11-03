// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A delegation's record in a [`DelegationJournal`] (ADR 0030).
//!
//! The record is written before the child's message is sent, so a supervisor
//! that dies between "sent" and "heard back" leaves something to find. The
//! child's id and stream position follow as they become known, and the
//! record is settled when the delegation ends, however it ends.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use a2a_protocol_types::MessageSendParams;
use a2a_protocol_types::delegation_journal::{DelegationJournal, DelegationRecord};
use tokio::time::Instant;

use super::{Delegation, DelegationOptions, Outcome};
use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};

/// The longest a recorded position lags the stream, apart from the moment the
/// child is first named and the moment the stream is lost, which are written
/// at once.
const PROGRESS_EVERY: Duration = Duration::from_secs(2);

/// The longest one write to the journal may hold up the delegation (audit
/// NEW-h-s0-3). The writes are awaited on the path events are read from, so
/// a journal database that stops answering would otherwise stop every
/// delegation of the supervisor, and the children's servers would see
/// slow consumers. A write that does not answer in this time is given up on
/// and logged, as one that fails is, and the next is tried after
/// [`PROGRESS_EVERY`].
pub(super) const WRITE_BOUND: Duration = Duration::from_secs(5);

/// What a delegation knows about its journal record.
pub(super) struct Journaled {
    journal: Arc<dyn DelegationJournal>,
    id: String,
    noted_task: Option<String>,
    noted_position: Option<String>,
    last_write: Option<Instant>,
    /// After a write failed or timed out, when the next may be tried.
    retry_at: Option<Instant>,
}

impl std::fmt::Debug for Journaled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Journaled")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Journaled {
    fn new(journal: Arc<dyn DelegationJournal>, id: String) -> Self {
        Self {
            journal,
            id,
            noted_task: None,
            noted_position: None,
            last_write: None,
            retry_at: None,
        }
    }
}

/// Records the delegation before its message is sent. A journal that does
/// not answer within [`WRITE_BOUND`] is an error, and nothing is sent.
pub(super) async fn begin(
    journal: &dyn DelegationJournal,
    record: &DelegationRecord,
) -> ClientResult<()> {
    tokio::time::timeout(WRITE_BOUND, journal.begin(record))
        .await
        .map_or_else(
            |_| {
                Err(ClientError::Timeout(format!(
                    "the delegation journal did not record the delegation within \
                     {WRITE_BOUND:?}; nothing was sent"
                )))
            },
            |written| written.map_err(ClientError::Protocol),
        )
}

/// Unix milliseconds now; `0` if the clock is before the epoch.
pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// The short label a journal stores for how a delegation ended.
pub(super) const fn label(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Completed(_) => "completed",
        Outcome::Failed { .. } => "failed",
        Outcome::Canceled(_) => "canceled",
        Outcome::Interrupted(_) => "interrupted",
        Outcome::Message(_) => "message",
        Outcome::Lost(_) => "lost",
        Outcome::Unreachable => "unreachable",
        Outcome::CancelRequested(_) => "cancel_requested",
        Outcome::CancelFailed(_) => "cancel_failed",
        Outcome::HandedOff => "handed_off",
    }
}

/// A new record for a delegation about to be sent: owned by `owner`, naming
/// `agent`, with the message's tenant and idempotency key.
pub(super) fn new_record(
    owner: impl Into<String>,
    agent: impl Into<String>,
    tenant: Option<String>,
    key: Option<String>,
) -> DelegationRecord {
    DelegationRecord::new(uuid::Uuid::new_v4().to_string(), owner, agent, now_ms())
        .with_tenant(tenant)
        .with_idempotency_key(key)
}

impl Delegation {
    /// [`start`](Self::start), recorded in `journal` first.
    ///
    /// The record (owner, the agent's URL, the tenant, the message's
    /// idempotency key) is written **before** the message is sent. If the
    /// journal cannot store it, nothing is sent. The child's id and stream
    /// position are noted as events arrive, at most every two seconds, and
    /// the record is settled with the delegation's outcome when it ends.
    ///
    /// A supervisor that restarts lists its open records
    /// ([`DelegationJournal::open`]) and picks each up: with
    /// [`reattach`](Self::reattach) and [`with_journal`](Self::with_journal)
    /// when the record names the child, or by sending the same message again
    /// under the record's idempotency key when it does not. The key is
    /// recorded only when the message carries one, which [`start`](Self::start)
    /// adds for an agent known to honour keys; set one on the message for any
    /// other.
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start); also the journal's error, as
    /// [`ClientError::Protocol`], when the record cannot be written. When the
    /// agent refuses the request the record is settled `not_started`: no
    /// child exists.
    pub async fn start_journaled(
        client: Arc<A2aClient>,
        params: MessageSendParams,
        journal: Arc<dyn DelegationJournal>,
        owner: impl Into<String> + Send,
        agent: impl Into<String> + Send,
    ) -> ClientResult<Self> {
        let options = DelegationOptions::new().with_journal(journal, owner, agent);
        Self::start_with(client, params, options).await
    }

    /// Keeps `record_id`, a record in `journal`, up to date from here on:
    /// for a delegation picked up with [`reattach`](Self::reattach) from a
    /// record [`DelegationJournal::open`] listed.
    #[must_use]
    pub fn with_journal(
        mut self,
        journal: Arc<dyn DelegationJournal>,
        record_id: impl Into<String>,
    ) -> Self {
        self.journal = Some(Journaled::new(journal, record_id.into()));
        self
    }

    /// The id of this delegation's journal record, if it has one.
    #[must_use]
    pub fn journal_id(&self) -> Option<&str> {
        self.journal.as_ref().map(|j| j.id.as_str())
    }

    /// Writes the child's id and position if they moved: at once when `force`
    /// or when the child was just named, otherwise at most every
    /// [`PROGRESS_EVERY`]. A write that fails or does not answer within
    /// [`WRITE_BOUND`] is logged and dropped, and none is tried again for
    /// [`PROGRESS_EVERY`] unless `force`: the delegation matters more than
    /// its record.
    pub(super) async fn note_progress(&mut self, force: bool) {
        let Some(j) = self.journal.as_mut() else {
            return;
        };
        let task_moved = self.task_id != j.noted_task;
        let position_moved = self.last_event_id != j.noted_position;
        if !task_moved && !position_moved {
            return;
        }
        let ripe = task_moved || j.last_write.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY);
        let waiting = j.retry_at.is_some_and(|at| Instant::now() < at);
        if !force && (!ripe || waiting) {
            return;
        }
        let written = tokio::time::timeout(
            WRITE_BOUND,
            j.journal.progress(
                &j.id,
                self.task_id.as_deref(),
                self.last_event_id.as_deref(),
                now_ms(),
            ),
        )
        .await;
        match written {
            Ok(Ok(())) => {
                j.noted_task.clone_from(&self.task_id);
                j.noted_position.clone_from(&self.last_event_id);
                j.last_write = Some(Instant::now());
            }
            Ok(Err(_e)) => {
                j.retry_at = Some(Instant::now() + PROGRESS_EVERY);
                trace_warn!(record = %j.id, error = %_e, "delegation journal: progress not recorded");
            }
            Err(_) => {
                j.retry_at = Some(Instant::now() + PROGRESS_EVERY);
                trace_warn!(record = %j.id, "delegation journal: progress not recorded in time");
            }
        }
    }

    /// Settles the record with `outcome`. A write that fails or does not
    /// answer within [`WRITE_BOUND`] is logged: the record stays open, and a
    /// restarted supervisor will find the child already settled when it
    /// reattaches. The two writes at the end, the last position and the
    /// outcome, hold the delegation up for twice the bound at most.
    pub(super) async fn note_settled(&mut self, outcome: &Outcome) {
        self.note_progress(true).await;
        let Some(j) = self.journal.take() else {
            return;
        };
        match tokio::time::timeout(
            WRITE_BOUND,
            j.journal.settle(&j.id, label(outcome), now_ms()),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(_e)) => {
                trace_warn!(record = %j.id, error = %_e, "delegation journal: outcome not recorded");
            }
            Err(_) => {
                trace_warn!(record = %j.id, "delegation journal: outcome not recorded in time");
            }
        }
    }
}

#[cfg(test)]
mod tests;
