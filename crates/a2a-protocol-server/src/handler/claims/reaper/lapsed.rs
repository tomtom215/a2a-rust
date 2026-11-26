// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Ends a task parked at `input-required` or `auth-required` whose lease
//! lapsed (COR-08, E03 R8).
//!
//! A leased turn is watched while it runs (`handler::lease`), and its
//! watcher stops it once the lease lapses. A turn that parks ends, and its
//! watch with it; the lease stays for the turn that continues the task. So a
//! child whose parent died while it waited for an answer stayed parked: its
//! deadline was read by nobody until the parent continued it, and that
//! parent was gone.
//!
//! Each look therefore also asks the lease store for lapsed leases
//! ([`LeaseStore::lapsed_leases`]). For each one whose task no turn here
//! runs, it takes the task's claim (the lease is kept under the claim's
//! scope, the task's partition), so no continuation starts meanwhile and no
//! second replica ends the task too, and reads the task and the lease again
//! under it. A task still parked behind a lapsed lease is canceled as
//! `CancelTask` cancels one: the executor's `cancel` hook is run, the status
//! is stored `canceled` with the cause `lease-lapsed` (extension
//! `cancel/v1`), and the ending is written to the task's log (and so to its
//! audit chain) and sent to its webhooks. Then the lease and the claim are
//! released.
//!
//! A lapsed lease of a task that is running is left alone: the turn's own
//! watcher stops it, or, if its replica died, the abandoned-task pass ends
//! it. One of a task that has finished, or no longer exists, is left for the
//! sweep, which keeps a lapsed row for [`LEASE_SWEEP_MARGIN`] so an operator
//! reading the table after an incident still finds it; the list puts the
//! most recently lapsed first, so those rows do not crowd out a lease that
//! has just lapsed.
//!
//! [`LEASE_SWEEP_MARGIN`]: super::LEASE_SWEEP_MARGIN

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::cancel_cause::CancelCause;
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole};
use a2a_protocol_types::task::{Task, TaskId, TaskState, TaskStatus};

use super::super::TurnClaims;
use super::{BATCH, Reaper};
use crate::handler::ExecutorTurn;
use crate::handler::messaging::terminal::TerminalTracking;
use crate::request_context::RequestContext;
use crate::store::LeaseStore;
use crate::store::lease::now_unix_ms;
use crate::store::tenant::TenantContext;

impl Reaper {
    /// Ends each parked task whose lease lapsed; see the module
    /// documentation.
    pub(super) async fn end_lapsed_parked(&self, claims: &TurnClaims) {
        let Some(leases) = &self.leases else { return };
        let found = match leases.lapsed_leases(now_unix_ms(), BATCH).await {
            Ok(found) => found,
            Err(_e) => {
                trace_warn!(error = %_e, "lapsed leases not listed; retrying next look");
                return;
            }
        };
        for (partition, task_id) in found {
            // A turn here runs the task: its watcher stops it.
            if claims.holds_task(&task_id) {
                continue;
            }
            self.end_parked(claims, &**leases, &partition, &task_id)
                .await;
        }
    }

    /// Takes the claim of one task with a lapsed lease, ends it if it is
    /// still parked, and releases the claim.
    async fn end_parked(
        &self,
        claims: &TurnClaims,
        leases: &dyn LeaseStore,
        partition: &str,
        task_id: &str,
    ) {
        let token = uuid::Uuid::new_v4().to_string();
        match claims
            .store
            .claim(partition, task_id, &claims.owner, &token, claims.ttl)
            .await
        {
            Ok(true) => {}
            // A turn runs it (its watcher stops it) or a send continues it.
            Ok(false) => return,
            Err(_e) => {
                trace_warn!(task_id, error = %_e, "parked task with a lapsed lease not claimed");
                return;
            }
        }
        let owed = TenantContext::scope_partition(partition, &token, async {
            self.settle_reservations(task_id).await;
            self.cancel_parked(leases, task_id, claims.ttl).await
        })
        .await;
        // Ended, and its ending not durable: the claim stays, and the next
        // look completes the ending of the now finished task (N05).
        if owed {
            return;
        }
        if let Err(_e) = claims.store.release(&token).await {
            trace_warn!(task_id, error = %_e, "reaper's claim not released; it expires");
        }
    }

    /// Cancels the task if it is parked and its lease has lapsed: whether its
    /// ending is still owed (stored, not durably logged and queued).
    async fn cancel_parked(&self, leases: &dyn LeaseStore, task_id: &str, bound: Duration) -> bool {
        let id = TaskId::new(task_id);
        let task = match self.task_store.get(&id).await {
            Ok(Some(task)) => task,
            // Gone (deleted, or purged by `parked_max_age`): the sweep takes
            // its lease.
            Ok(None) => return false,
            Err(_e) => {
                trace_warn!(task_id, error = %_e, "parked task with a lapsed lease not read");
                return false;
            }
        };
        if !task.status.state.is_interrupted() {
            return false;
        }
        // Read again under the claim: a continuation granted a new lease, or
        // a renewal moved the deadline, since the list was read.
        match leases.lapsed(&TenantContext::partition(), task_id).await {
            Ok(Some(true)) => {}
            Ok(_) => return false,
            Err(_e) => {
                trace_warn!(task_id, error = %_e, "lapsed lease not read again; the task stays parked");
                return false;
            }
        }
        self.run_cancel_hook(&task, bound).await;
        let mut ended = task;
        ended.status = TaskStatus::with_timestamp(TaskState::Canceled);
        crate::handler::cancel_cause::stamp_status(
            &mut ended.status,
            &ended.id,
            &ended.context_id,
            Some(CancelCause::LeaseLapsed),
        );
        // The terminal-state rule refuses it if the task finished meanwhile,
        // and the fence if a send took the claim over since.
        if let Err(_e) = self.task_store.save_status_delta(&ended).await {
            trace_warn!(task_id, error = %_e, "parked task with a lapsed lease not ended");
            return false;
        }
        trace_warn!(task_id, "lease lapsed; ended the parked task: canceled");
        self.metrics
            .on_lease(crate::metrics::control::lease::LAPSED);
        self.release_lease(task_id).await;
        let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
            id.clone(),
            ended.context_id.clone(),
            ended.status,
        ));
        !self.announce(&id, &event).await
    }

    /// Runs the executor's `cancel` hook for `task`, as `CancelTask` does for
    /// a task no turn runs, bounded by `bound` (the claim's time to live, so
    /// a hook that never returns does not stop every later look). What the
    /// hook writes goes to a live queue of the task if one is open, else
    /// nowhere; the store's status is written by the caller. A hook that
    /// refuses is logged and the task is canceled all the same: a lapse
    /// stops a running turn without asking its executor either.
    async fn run_cancel_hook(&self, task: &Task, bound: Duration) {
        let turn = ExecutorTurn::default();
        turn.lapse_lease();
        let writer = TerminalTracking::new(
            self.queues.writer_for_cancel(&task.id).await,
            Arc::new(turn),
        );
        let ctx = RequestContext::new(
            Message::new(
                MessageId::new(uuid::Uuid::new_v4().to_string()),
                MessageRole::User,
                vec![],
            )
            .with_task_id(task.id.clone())
            .with_context_id(task.context_id.clone()),
            task.id.clone(),
            task.context_id.0.clone(),
        );
        match tokio::time::timeout(bound, writer.cancel_with(&*self.executor, &ctx)).await {
            Ok(Ok(())) => {}
            Ok(Err(_e)) => {
                trace_warn!(task_id = %task.id, error = %_e, "the executor's cancel hook failed; the task is canceled");
            }
            Err(_) => {
                trace_warn!(task_id = %task.id, "the executor's cancel hook did not return in time; the task is canceled");
            }
        }
    }
}
