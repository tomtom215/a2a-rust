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
//!
//! # The task row (release audit F9)
//!
//! A new task's row is saved before its inline push config is registered,
//! because the config store checks that the task exists; a drop in that await
//! left the row `Submitted` with no executor for ever (the retention sweeps
//! delete only terminal tasks), visible to `GetTask` and `ListTasks`, and the
//! released key let the client's retry create a second task for one send. A
//! saved new row is now deleted with the push configs registered for it.
//!
//! The deletion holds the per-context lock until it is done: the guard owns
//! the lock, so a concurrent send for the same context cannot find the task by
//! context in the window between the drop and the deletion. A continuation's
//! row is saved with nothing awaited after it (the approval's audit record is
//! written before the save), so a drop cannot leave a turn half-committed.
//!
//! # The tenant
//!
//! The release runs in a spawned task, and the tenant-aware stores read the
//! tenant and caller from task-locals a spawned task does not inherit. They
//! are captured when the guard is made and put back around the release; until
//! 2026-10-18 a dropped send under a tenant released its key in the empty
//! tenant's partition and kept it held in its own.

use std::sync::Arc;

use a2a_protocol_types::task::TaskId;
use tokio::sync::OwnedMutexGuard;

use super::super::ExecutorTurn;
use crate::push::PushConfigStore;
use crate::store::TaskStore;
use crate::store::tenant::{CapturedTenant, TenantContext};
use crate::streaming::EventQueueManager;

use super::execute::CancellationTokens;

/// What one send has taken so far, released on drop unless disarmed.
pub(super) struct CommitGuard {
    task_id: TaskId,
    queues: EventQueueManager,
    tokens: CancellationTokens,
    store: Arc<dyn TaskStore>,
    push_store: Arc<dyn PushConfigStore>,
    /// The idempotency key this send claimed, if any.
    key: Option<String>,
    /// The per-context lock, held until the commit no longer needs it or,
    /// on a drop, until the release is done.
    context: Option<OwnedMutexGuard<()>>,
    /// The tenant and caller the stores are scoped to.
    tenant: Option<CapturedTenant>,
    /// Set once this send has saved a *new* task's row.
    created_row: bool,
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
        push_store: Arc<dyn PushConfigStore>,
        key: Option<String>,
        context: OwnedMutexGuard<()>,
    ) -> Self {
        Self {
            task_id,
            queues,
            tokens,
            store,
            push_store,
            key,
            context: Some(context),
            tenant: Some(TenantContext::capture()),
            created_row: false,
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

    /// Records that this send saved a new task's row.
    pub(super) const fn created_row(&mut self) {
        self.created_row = true;
    }

    /// Releases the per-context lock: the task is committed far enough that
    /// a concurrent send for the context may find it.
    pub(super) fn release_context(&mut self) {
        self.context = None;
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
        let push_store = Arc::clone(&self.push_store);
        let created_row = self.created_row;
        let context = self.context.take();
        let tenant = self.tenant.take();
        trace_warn!(
            task_id = %task_id,
            "send dropped before its executor started; releasing what it took"
        );
        // Spawned because `Drop` cannot await, in the request's tenant scope.
        // The order mirrors the error paths: the row, queue, token, key.
        let release = async move {
            if created_row {
                // Nothing outside this send has seen the task: the context
                // lock is still held, and the executor never started.
                if let Ok(configs) = push_store.list(&task_id.0).await {
                    for config in configs {
                        if let Some(id) = config.id {
                            let _ = push_store.delete(&task_id.0, &id).await;
                        }
                    }
                }
                let _ = store.delete(&task_id).await;
            }
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
            // Only now may a concurrent send for this context look for its
            // task.
            drop(context);
        };
        tokio::spawn(async move {
            match tenant {
                Some(tenant) => tenant.scope(release).await,
                None => release.await,
            }
        });
    }
}
