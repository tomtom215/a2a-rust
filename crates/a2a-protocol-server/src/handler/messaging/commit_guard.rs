// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Releasing what a send took when its request is dropped mid-commit (N26).
//!
//! hyper drops a request's future when its client goes away, so every await
//! between claiming an idempotency key and spawning the executor is a point
//! at which the send can simply stop. Each step that can *fail* there
//! releases what it holds before returning its error; a drop runs none of
//! that code. Before this guard a dropped send kept its queue lease and its
//! cancellation token for good — every later continuation of the task was
//! refused as "already being processed", and the queue counted against
//! `max_concurrent_queues` for the life of the process.
//!
//! The guard is disarmed as soon as the commit returns, success or failure:
//! on success the spawned executor owns the release (see `execute`), and on
//! failure the failing step already released. So it fires only on a drop.

use std::sync::Arc;

use a2a_protocol_types::task::TaskId;

use super::super::ExecutorTurn;
use crate::store::TaskStore;
use crate::streaming::EventQueueManager;

use super::execute::CancellationTokens;

/// What one send has taken so far, released on drop unless disarmed.
pub(super) struct CommitGuard {
    task_id: TaskId,
    queues: EventQueueManager,
    tokens: CancellationTokens,
    store: Arc<dyn TaskStore>,
    /// The idempotency key this send claimed, if any.
    key: Option<String>,
    /// Set once this send's queue lease exists.
    leased: bool,
    /// This send's token entry, identified by its turn: only an entry whose
    /// turn is this one is removed, never a token another turn registered
    /// under the same id — the N21 wait can leave a send waiting beside the
    /// previous turn's executor, and dropping it there must not release that
    /// executor's token.
    turn: Option<Arc<ExecutorTurn>>,
    armed: bool,
}

impl CommitGuard {
    pub(super) fn new(
        task_id: TaskId,
        queues: EventQueueManager,
        tokens: CancellationTokens,
        store: Arc<dyn TaskStore>,
        key: Option<String>,
    ) -> Self {
        Self {
            task_id,
            queues,
            tokens,
            store,
            key,
            leased: false,
            turn: None,
            armed: true,
        }
    }

    /// Records that this send now holds the task's queue lease.
    pub(super) const fn leased(&mut self) {
        self.leased = true;
    }

    /// Records the token entry this send registered.
    pub(super) fn registered(&mut self, turn: &Arc<ExecutorTurn>) {
        self.turn = Some(Arc::clone(turn));
    }

    /// The commit returned; whatever it holds is someone else's to release.
    pub(super) const fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CommitGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let task_id = self.task_id.clone();
        let queues = self.queues.clone();
        let tokens = Arc::clone(&self.tokens);
        let store = Arc::clone(&self.store);
        let key = self.key.take();
        let leased = self.leased;
        let turn = self.turn.take();
        trace_warn!(
            task_id = %task_id,
            "send dropped before its executor started; releasing what it took"
        );
        // Spawned because `Drop` cannot await. The order mirrors the error
        // paths: queue, then token, then key.
        tokio::spawn(async move {
            if leased {
                queues.destroy(&task_id).await;
            }
            if let Some(turn) = turn {
                let mut map = tokens.write().await;
                if map
                    .get(&task_id)
                    .is_some_and(|e| Arc::ptr_eq(&e.turn, &turn))
                {
                    map.remove(&task_id);
                }
            }
            if let Some(key) = key {
                let _ = store.release_idempotency_key(&key).await;
            }
        });
    }
}
