// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A delegation's record in a [`DelegationJournal`] (ADR 0030).
//!
//! The record is written before the child's message is sent, so a supervisor
//! that dies between "sent" and "heard back" leaves something to find. The
//! child's id and stream position follow as they become known, and the
//! record is settled when the delegation ends, however it ends.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use a2a_protocol_types::MessageSendParams;
use a2a_protocol_types::delegation_journal::{DelegationJournal, DelegationRecord};
use a2a_protocol_types::idempotency;

use super::{Delegation, Outcome};
use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};

/// The longest a recorded position lags the stream, apart from the moment the
/// child is first named and the moment the stream is lost, which are written
/// at once.
const PROGRESS_EVERY: Duration = Duration::from_secs(2);

/// What a delegation knows about its journal record.
pub(super) struct Journaled {
    journal: Arc<dyn DelegationJournal>,
    id: String,
    noted_task: Option<String>,
    noted_position: Option<String>,
    last_write: Option<Instant>,
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
        }
    }
}

/// Unix milliseconds now; `0` if the clock is before the epoch.
fn now_ms() -> i64 {
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
    }
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
        mut params: MessageSendParams,
        journal: Arc<dyn DelegationJournal>,
        owner: impl Into<String> + Send,
        agent: impl Into<String> + Send,
    ) -> ClientResult<Self> {
        if client.honours_idempotency() && matches!(idempotency::key_of(&params.message), Ok(None))
        {
            // A fresh key is 64 hex digits, always valid, so this cannot fail.
            let _ = idempotency::set_key(&mut params.message, &super::fresh_key());
        }
        let key = idempotency::key_of(&params.message)
            .ok()
            .flatten()
            .map(str::to_owned);
        let record =
            DelegationRecord::new(uuid::Uuid::new_v4().to_string(), owner, agent, now_ms())
                .with_tenant(params.tenant.clone())
                .with_idempotency_key(key);
        journal
            .begin(&record)
            .await
            .map_err(ClientError::Protocol)?;
        match Self::start(client, params).await {
            Ok(this) => Ok(this.with_journal(journal, record.id)),
            Err(e) => {
                if super::refused(&e) {
                    let _ = journal.settle(&record.id, "not_started", now_ms()).await;
                }
                Err(e)
            }
        }
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
    /// [`PROGRESS_EVERY`]. A write that fails is logged and dropped: the
    /// delegation matters more than its record.
    pub(super) async fn note_progress(&mut self, force: bool) {
        let Some(j) = self.journal.as_mut() else {
            return;
        };
        let task_moved = self.task_id != j.noted_task;
        let position_moved = self.last_event_id != j.noted_position;
        if !task_moved && !position_moved {
            return;
        }
        let due =
            force || task_moved || j.last_write.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY);
        if !due {
            return;
        }
        let written = j
            .journal
            .progress(
                &j.id,
                self.task_id.as_deref(),
                self.last_event_id.as_deref(),
                now_ms(),
            )
            .await;
        match written {
            Ok(()) => {
                j.noted_task.clone_from(&self.task_id);
                j.noted_position.clone_from(&self.last_event_id);
                j.last_write = Some(Instant::now());
            }
            Err(_e) => {
                trace_warn!(record = %j.id, error = %_e, "delegation journal: progress not recorded");
            }
        }
    }

    /// Settles the record with `outcome`. A write that fails is logged: the
    /// record stays open, and a restarted supervisor will find the child
    /// already settled when it reattaches.
    pub(super) async fn note_settled(&mut self, outcome: &Outcome) {
        self.note_progress(true).await;
        let Some(j) = self.journal.take() else {
            return;
        };
        if let Err(_e) = j.journal.settle(&j.id, label(outcome), now_ms()).await {
            trace_warn!(record = %j.id, error = %_e, "delegation journal: outcome not recorded");
        }
    }
}
