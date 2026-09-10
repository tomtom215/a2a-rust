// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Registering a task's cancellation token, with the stale-token sweep that
//! keeps the map bounded.
//!
//! The token goes in BEFORE the task is saved (FIX(#8)): a task that exists
//! in the store with no token has a window in which a concurrent
//! `CancelTask` silently fails to cancel. The sweep runs first, in three
//! phases, so the map never grows past [`HandlerLimits::max_cancellation_tokens`]
//! by more than the entry being added:
//!
//! 1. collect candidates under the READ lock;
//! 2. remove the ones that are still stale under a brief WRITE lock;
//! 3. insert the new token under a WRITE lock.
//!
//! [`HandlerLimits::max_cancellation_tokens`]: crate::handler::HandlerLimits::max_cancellation_tokens

use std::time::Instant;

use a2a_protocol_types::task::TaskId;
use tokio_util::sync::CancellationToken;

use super::super::{CancellationEntry, RequestHandler};
use super::decisions::{evict_aged_token, token_aged, token_still_evictable};

impl RequestHandler {
    /// Sweeps stale tokens if the map is at capacity, then inserts `token`
    /// for `task_id`.
    pub(super) async fn register_cancellation_token(
        &self,
        task_id: &TaskId,
        token: CancellationToken,
    ) {
        // Phase 1.
        let (cancelled_ids, aged_candidates) = self.collect_stale_candidates().await;
        let mut stale_ids = cancelled_ids;
        stale_ids.extend(self.confirm_aged_candidates(aged_candidates).await);

        // Phase 2.
        if !stale_ids.is_empty() {
            self.evict_stale_tokens(&stale_ids).await;
        }

        // Phase 3: Insert the new token under WRITE lock.
        let mut tokens = self.cancellation_tokens.write().await;
        tokens.insert(
            task_id.clone(),
            CancellationEntry {
                token,
                created_at: Instant::now(),
            },
        );
    }

    /// Phase 1: collects `(cancelled, aged)` candidate ids under the READ
    /// lock — non-blocking for other readers, which avoids holding a write
    /// lock during the O(n) sweep of all cancellation tokens.
    ///
    /// Both lists are empty while the map is below
    /// `max_cancellation_tokens`. Cancelled tokens are always evictable. An
    /// *aged* but not-cancelled token may still belong to a live,
    /// long-running executor; evicting it would make that task uncancelable,
    /// so aged candidates are only evicted once
    /// [`confirm_aged_candidates`](Self::confirm_aged_candidates) finds their
    /// event queue gone.
    async fn collect_stale_candidates(&self) -> (Vec<TaskId>, Vec<TaskId>) {
        let tokens = self.cancellation_tokens.read().await;
        if tokens.len() >= self.limits.max_cancellation_tokens {
            let now = Instant::now();
            let mut cancelled = Vec::new();
            let mut aged = Vec::new();
            for (id, entry) in tokens.iter() {
                if entry.token.is_cancelled() {
                    cancelled.push(id.clone());
                } else if token_aged(
                    now.duration_since(entry.created_at),
                    self.limits.max_token_age,
                ) {
                    aged.push(id.clone());
                }
            }
            drop(tokens);
            (cancelled, aged)
        } else {
            (Vec::new(), Vec::new())
        }
    }

    /// Keeps only the aged candidates whose event queue is no longer
    /// registered — i.e. the executor has finished but the token lingered. A
    /// token whose queue is still live is left in place so the task remains
    /// cancelable.
    async fn confirm_aged_candidates(&self, aged: Vec<TaskId>) -> Vec<TaskId> {
        let mut evictable = Vec::new();
        for id in aged {
            let queue_live = self.event_queue_manager.has_queue(&id).await;
            if evict_aged_token(queue_live) {
                evictable.push(id);
            }
        }
        evictable
    }

    /// Phase 2: removes the candidates under a brief WRITE lock, re-validating
    /// each at removal time — a concurrent send may have replaced the entry
    /// with a fresh live token since the read-lock scan (see
    /// [`token_still_evictable`]).
    async fn evict_stale_tokens(&self, stale_ids: &[TaskId]) {
        let now = Instant::now();
        let mut tokens = self.cancellation_tokens.write().await;
        for id in stale_ids {
            let evict = tokens
                .get(id)
                .is_some_and(|e| token_still_evictable(e, now, self.limits.max_token_age));
            if evict {
                tokens.remove(id);
            }
        }
    }
}
