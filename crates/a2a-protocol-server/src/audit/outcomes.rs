// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Records a required log must not lose: spooling, draining and reconciling
//! (adversarial review A3, 2026-11-12).
//!
//! [`AuditLog::require_record`] used to gate admission on the `call.started`
//! record and nothing after it. When the store refused the `call` record that
//! says how the call ended, or a `task.event`, the failure was counted and the
//! record was gone: a chain proves the records it holds are intact and cannot
//! show that one was never written.
//!
//! In a required log a record the store refuses is **spooled** and written
//! when the store answers again. Three things keep the promise honest:
//!
//! * the spool is bounded; a record that does not fit is counted in
//!   [`AuditLog::outcomes_lost`], never dropped silently;
//! * a start with no end, which a crash leaves behind and a full spool
//!   can, is found by [`AuditLog::unresolved_calls`] and closed with a
//!   `call.unresolved` record by [`AuditLog::reconcile_unresolved`];
//! * the spool is memory. A process that dies with records in it loses them,
//!   which is what reconciliation is for, and what the docs say.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use a2a_protocol_types::audit::{AuditRecord, kind};
use a2a_protocol_types::error::{A2aError, A2aResult};

use super::log::AuditLog;
use super::store::record_millis;

/// Records a log holds for a store that is refusing them. A record past it is
/// counted as lost.
pub(super) const MAX_PENDING: usize = 1_024;

/// Tries a spooled record makes before the store is asked whether it is the
/// record that is refused, or the store.
const POISON_AFTER: u32 = 8;

/// The wait before a retry, doubled up to [`RETRY_CEILING`].
const RETRY_FLOOR: Duration = Duration::from_millis(100);
const RETRY_CEILING: Duration = Duration::from_secs(5);

/// A spooled record and how many times the store has refused it.
#[derive(Debug)]
struct Spooled {
    record: AuditRecord,
    refused: u32,
}

/// The state a required log keeps for records the store has refused.
#[derive(Debug, Default)]
pub(super) struct Outcomes {
    /// Ids of calls admitted and not yet ended in this process.
    in_flight: Mutex<HashSet<String>>,
    pending: Mutex<VecDeque<Spooled>>,
    /// Held by whoever is writing the spool; drains are one at a time, so
    /// the front of the queue is the same record before and after a write.
    draining: tokio::sync::Mutex<()>,
    /// A retry task exists.
    retrying: AtomicBool,
    lost: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A call that was admitted and has no record of how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnresolvedCall {
    /// The chain the call belongs to.
    pub chain: String,
    /// The position of its `call.started` record.
    pub started_seq: u64,
    /// The call's id ([`CallContext::call_id`](crate::CallContext::call_id)).
    pub call_id: String,
    /// The method it called.
    pub method: Option<String>,
}

fn call_id_of(record: &AuditRecord) -> Option<&str> {
    record.detail.get("callId").and_then(|v| v.as_str())
}

impl AuditLog {
    /// Records waiting for the store to take them; see
    /// [`require_record`](Self::require_record).
    #[must_use]
    pub fn pending_outcomes(&self) -> usize {
        lock(&self.outcomes.pending).len()
    }

    /// Records a required log could not keep: the spool was full, or the
    /// record was the one the store kept refusing. Each is a call, event or
    /// decision whose record does not exist and which no later write will
    /// supply; [`reconcile_unresolved`](Self::reconcile_unresolved) closes
    /// the calls among them. Zero in a healthy deployment.
    #[must_use]
    pub fn outcomes_lost(&self) -> u64 {
        self.outcomes.lost.load(Ordering::Relaxed)
    }

    pub(super) fn admit(&self, call_id: &str) {
        lock(&self.outcomes.in_flight).insert(call_id.to_owned());
    }

    pub(super) fn end_call(&self, call_id: &str) {
        lock(&self.outcomes.in_flight).remove(call_id);
    }

    /// Appends `record`, and spools it if the store refuses.
    ///
    /// The write runs in a task of its own, so a caller that is dropped while
    /// it waits (a client that hung up, a deadline) does not abandon a record
    /// the store may already hold, nor one that was about to be spooled.
    ///
    /// # Errors
    ///
    /// The store's refusal; the record is then in the spool.
    pub(crate) async fn append_required(
        self: &Arc<Self>,
        record: AuditRecord,
    ) -> A2aResult<AuditRecord> {
        let log = Arc::clone(self);
        tokio::spawn(async move {
            match log.append(record.clone()).await {
                Ok(sealed) => Ok(sealed),
                Err(e) => {
                    log.spool(record);
                    Err(e)
                }
            }
        })
        .await
        .unwrap_or_else(|e| Err(A2aError::internal(format!("an audit write failed: {e}"))))
    }

    fn spool(self: &Arc<Self>, record: AuditRecord) {
        {
            let mut pending = lock(&self.outcomes.pending);
            if pending.len() >= MAX_PENDING {
                drop(pending);
                self.outcomes.lost.fetch_add(1, Ordering::Relaxed);
                trace_error!(kind = %record.kind, "audit: the spool is full, a record is lost");
                return;
            }
            pending.push_back(Spooled { record, refused: 1 });
        }
        self.ensure_retrying();
    }

    /// Writes the spool, oldest first, until the store refuses one or none is
    /// left; returns how many remain. A drain already running counts as the
    /// records it has not yet written.
    ///
    /// Runs in a task of its own, so dropping the caller does not stop it
    /// between the store taking a record and the spool forgetting it.
    pub async fn flush_pending(self: &Arc<Self>) -> usize {
        if self.pending_outcomes() == 0 {
            return 0;
        }
        let log = Arc::clone(self);
        tokio::spawn(async move { log.drain().await })
            .await
            .unwrap_or_else(|_| self.pending_outcomes())
    }

    async fn drain(&self) -> usize {
        let Ok(_one) = self.outcomes.draining.try_lock() else {
            return self.pending_outcomes();
        };
        loop {
            let Some(record) = lock(&self.outcomes.pending)
                .front()
                .map(|s| s.record.clone())
            else {
                return 0;
            };
            if self.append(record).await.is_ok() {
                lock(&self.outcomes.pending).pop_front();
                continue;
            }
            if self.give_up_on_head().await {
                continue;
            }
            return self.pending_outcomes();
        }
    }

    /// Whether the record at the front has been refused enough times, by a
    /// store that answers reads, that it is the record at fault: it is
    /// counted lost and removed, so it does not hold back the ones behind it.
    /// A store that does not answer is not at fault in that sense; its
    /// records wait.
    async fn give_up_on_head(&self) -> bool {
        let Some(chain) = self.count_refusal_of_head() else {
            return false;
        };
        if self.store().head(&chain).await.is_err() {
            return false;
        }
        let _gone = lock(&self.outcomes.pending).pop_front();
        self.outcomes.lost.fetch_add(1, Ordering::Relaxed);
        trace_error!(
            kind = _gone.as_ref().map_or("", |s| s.record.kind.as_str()),
            "audit: the store refuses this record and answers reads; it is lost"
        );
        true
    }

    /// Counts one more refusal of the record at the front; its chain once it
    /// has been refused [`POISON_AFTER`] times.
    fn count_refusal_of_head(&self) -> Option<String> {
        let mut pending = lock(&self.outcomes.pending);
        let head = pending.front_mut()?;
        head.refused += 1;
        let chain = (head.refused >= POISON_AFTER).then(|| head.record.chain.clone());
        drop(pending);
        chain
    }

    /// Starts the task that retries the spool, unless one is running.
    fn ensure_retrying(self: &Arc<Self>) {
        if self.outcomes.retrying.swap(true, Ordering::SeqCst) {
            return;
        }
        let log = Arc::clone(self);
        drop(tokio::spawn(async move {
            let mut wait = RETRY_FLOOR;
            loop {
                tokio::time::sleep(wait).await;
                if log.drain().await > 0 {
                    wait = (wait * 2).min(RETRY_CEILING);
                    continue;
                }
                log.outcomes.retrying.store(false, Ordering::SeqCst);
                // A record spooled after the drain found none, whose own call
                // saw this task still running and started no other.
                if log.pending_outcomes() == 0 || log.outcomes.retrying.swap(true, Ordering::SeqCst)
                {
                    return;
                }
                wait = RETRY_FLOOR;
            }
        }));
    }

    /// The calls of `chain` after position `after` that were admitted at least
    /// `older_than` ago and have no record of how they ended.
    ///
    /// A call is unresolved when its `call.started` record names a call id
    /// that no `call` or `call.unresolved` record names, and the call is not
    /// still running in this process or waiting in its spool. A `call.started`
    /// written before call ids existed names none and is not reported.
    ///
    /// **`older_than` must exceed your longest request.** Another replica
    /// sharing the store may be serving a call this log cannot see, and its
    /// start looks the same as a dead one's until its outcome arrives.
    ///
    /// # Errors
    ///
    /// When the store cannot be read.
    pub async fn unresolved_calls(
        &self,
        chain: &str,
        after: u64,
        older_than: Duration,
    ) -> A2aResult<Vec<UnresolvedCall>> {
        // Snapshotted before the read: a call that ends meanwhile is in the
        // chain, and one that has not ended is excluded by this.
        let live = self.live_call_ids();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        let cutoff = i64::try_from(older_than.as_millis()).unwrap_or(i64::MAX);
        let mut started = Vec::new();
        let mut ended = HashSet::new();
        let mut cursor = after;
        loop {
            let page = self.store().read(chain, cursor, 1_000).await?;
            let Some(last) = page.last().map(|r| r.seq) else {
                break;
            };
            cursor = last;
            for r in page {
                match (r.kind.as_str(), call_id_of(&r)) {
                    (kind::CALL | kind::CALL_UNRESOLVED, Some(id)) => {
                        ended.insert(id.to_owned());
                    }
                    (kind::CALL_STARTED, Some(id)) => {
                        started.push((r.seq, id.to_owned(), r.method.clone(), record_millis(&r)));
                    }
                    _ => {}
                }
            }
        }
        Ok(started
            .into_iter()
            .filter(|(_, id, _, at)| {
                !ended.contains(id) && !live.contains(id) && now.saturating_sub(*at) >= cutoff
            })
            .map(|(started_seq, call_id, method, _)| UnresolvedCall {
                chain: chain.to_owned(),
                started_seq,
                call_id,
                method,
            })
            .collect())
    }

    pub(super) fn live_call_ids(&self) -> HashSet<String> {
        let mut ids = lock(&self.outcomes.in_flight).clone();
        ids.extend(
            lock(&self.outcomes.pending)
                .iter()
                .filter_map(|s| call_id_of(&s.record).map(str::to_owned)),
        );
        ids
    }

    /// Closes every call [`unresolved_calls`](Self::unresolved_calls) finds
    /// with a `call.unresolved` record, and returns the records it wrote.
    ///
    /// Run it at start-up, before serving, and on a schedule: it is how a
    /// crash that took records with it, or a spool that overflowed, becomes
    /// visible in the chain, so that an auditor reading it sees "the outcome
    /// of this call is not known" instead of nothing. It is safe to run twice
    /// and from several replicas: a call already closed is not closed again,
    /// and one closed twice at once reads as closed. It does not claim the
    /// call failed.
    ///
    /// # Errors
    ///
    /// When the store cannot be read or a record cannot be appended; the ones
    /// written before that stay written.
    pub async fn reconcile_unresolved(
        &self,
        chain: &str,
        after: u64,
        older_than: Duration,
    ) -> A2aResult<Vec<AuditRecord>> {
        let mut written = Vec::new();
        for call in self.unresolved_calls(chain, after, older_than).await? {
            let mut r = AuditRecord::new(chain, kind::CALL_UNRESOLVED);
            r.method = call.method;
            r.detail
                .insert("callId".to_owned(), call.call_id.clone().into());
            r.detail
                .insert("startedSeq".to_owned(), call.started_seq.into());
            written.push(self.append(r).await?);
        }
        Ok(written)
    }
}
