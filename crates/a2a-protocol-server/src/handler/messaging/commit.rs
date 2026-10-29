// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Committing a send: the tenant's slot, validation, then the task, its
//! queue, token, row and executor, under the per-context lock; and undoing
//! all of it when a step fails, so nothing outlives a refused send.

use a2a_protocol_types::params::MessageSendParams;
use tokio::sync::OwnedSemaphorePermit;

use super::super::RequestHandler;
use super::claimed::{Claimed, Regated};
use super::mode::SendMode;
use super::start::{Committed, ExecutorStart, Started, runtime_has_one_worker};
use super::{basis, commit_guard, create, idempotency, quorum};
use crate::call_context::CallContext;
use crate::error::ServerResult;

impl RequestHandler {
    /// Takes the tenant's concurrency slot, validates the request, and
    /// commits the task.
    ///
    /// Nothing with a side effect happens before validation is through, so
    /// a refused request leaves no queue, no task row and no cancellation
    /// token behind; [`commit_task`](Self::commit_task) is where side effects
    /// begin, and it releases what it admitted on every failure.
    ///
    /// The slot is taken first: a refused request must not cost the tenant
    /// the very resources the limit protects. The permit is moved into the
    /// spawned executor and released when that task ends, however it ends —
    /// which is why the commit is this method's tail expression.
    pub(super) async fn validate_and_commit(
        &self,
        mut params: MessageSendParams,
        streaming: bool,
        call_ctx: &CallContext,
    ) -> ServerResult<(SendMode, Committed)> {
        let tenant_slot = self.acquire_tenant_slot().await?;

        // SPEC §3.3.4: a streaming send is only permitted when the configured
        // agent card advertises `capabilities.streaming == true`. Reject with
        // UnsupportedOperationError otherwise. (No-op when no card is configured.)
        if streaming {
            self.ensure_streaming_supported()?;
        }

        // Before an approval is counted or a send key's replay answered.
        let content_check = self.validate_send_params(&mut params)?;

        let mode = SendMode::of(&params, streaming);
        self.commit_task(
            params,
            mode.use_background,
            tenant_slot,
            call_ctx,
            content_check,
        )
        .await
        .map(|started| (mode, started))
    }

    /// Resolves the task the message belongs to and commits it: the queue
    /// lease, the cancellation token, the store row, the inline push config,
    /// and finally the executor. Every failure after the lease releases the
    /// queue and token (see `create`), so nothing outlives a refused send.
    // 61 of an allowed 60, crossed by threading `call_ctx` through: one line
    // of signature and one of argument. The function's shape is unchanged,
    // and splitting it would separate the per-context lock from the work it
    // is held across, which is the one thing this function exists to keep
    // together.
    //
    // `significant_drop_tightening` (nursery) reads the per-context lock as
    // held to the end of the function; it is moved into the `CommitGuard`,
    // which releases it at the point the lock stops being needed, and the
    // lint does not follow moves into a struct.
    #[allow(clippy::too_many_lines, clippy::significant_drop_tightening)]
    async fn commit_task(
        &self,
        params: MessageSendParams,
        use_background: bool,
        tenant_slot: Option<OwnedSemaphorePermit>,
        call_ctx: &CallContext,
        content_check: crate::request_context::ContentCheck,
    ) -> ServerResult<Committed> {
        let (context_id, minted) = self.resolve_context_id(&params.message).await?;

        // The per-context lock serializes the find + save sequence for one
        // context_id, so two concurrent sends cannot both create a new task
        // for it. Held until the task is saved.
        let context_lock = self.keyed_lock(&context_id).await;
        // Owned, so a send dropped mid-commit can hold it until its rollback
        // is done (see `commit_guard`).
        let context_guard = context_lock.lock_owned().await;

        let stored_task = self
            .stored_for(&context_id, minted, &params.message)
            .await?;
        let resolution = self
            .resolve_task_id(&params.message, stored_task.as_ref())
            .await?;
        let task_id = resolution.id;
        // `continues` is `Some` only when the send names a live task in this
        // context other than the canonical one, which is the case
        // `find_task_by_context` cannot see. Everywhere else this is a no-op
        // and `stored_task` stays exactly what it was.
        let stored_task = self
            .settled_task(&task_id, resolution.continues.or(stored_task))
            .await?;
        // A decision is checked against the request pending on the task it
        // continues, before anything is claimed; see `crate::approval`. With
        // turn claims a quorum is counted under the claim (`claimed`).
        let count = self.turn_claims.is_none();
        let gated = self.gate_decision(stored_task.as_ref(), &params.message, call_ctx, count);
        let mut approval = match Box::pin(gated).await? {
            quorum::Gated::Pass(approval) => approval,
            quorum::Gated::Waiting(task) => return Ok(Committed::Waiting(task)),
        };

        // An idempotency key, if the send carries one, is claimed here: after
        // everything that can reject the request on its own terms, and before
        // the send's first side effect. Both halves matter. Claiming later
        // would let two racing duplicates each lease a queue before either
        // noticed the other; claiming earlier would burn a key on a request
        // that was never going to run. Every failure below releases it, the
        // same discipline the queue lease and cancellation token follow.
        // Boxed for the same reason the inline push-config branch below is: a
        // cold branch inlined here enlarges the send future for every send,
        // and all three dispatch futures sit just under clippy's
        // `large_futures` threshold.
        let claimed_key = match Box::pin(self.claim_send_key(&params.message, &task_id)).await? {
            idempotency::SendKey::Replay(task) => return Ok(Committed::Replay(task)),
            idempotency::SendKey::Claimed(key) => Some(key),
            idempotency::SendKey::Absent => None,
        };
        // Across replicas sharing the store, one turn of a task at a time
        // (`claims`): taken after a replay is answered, so a retried send
        // whose first attempt still runs replays rather than waits.
        let message = &params.message;
        let claimed = self.claimed_view(
            &task_id,
            stored_task,
            message,
            call_ctx,
            claimed_key.as_deref(),
        );
        let turn = match Box::pin(claimed).await? {
            Claimed::Turn(turn) => turn,
            Claimed::Waiting(task) => return Ok(Committed::Waiting(task)),
            Claimed::Replay(task) => return Ok(Committed::Replay(task)),
        };
        if let Regated::Yes(regated) = turn.regated {
            approval = *regated;
        }
        let (claim, stored_task, claim_read_at) = (turn.claim, turn.stored, turn.read_at);
        // Released on drop until the commit returns (N26): see `commit_guard`.
        let mut guard = commit_guard::CommitGuard::new(
            task_id.clone(),
            self.event_queue_manager.clone(),
            std::sync::Arc::clone(&self.cancellation_tokens),
            std::sync::Arc::clone(&self.task_store),
            std::sync::Arc::clone(&self.push_config_store),
            claimed_key.clone(),
            context_guard,
        );
        let guard_ref = &mut guard;

        // Boxed: this block holds the whole creation path's locals, and
        // inlining it here puts the JSON-RPC and REST dispatch futures over
        // clippy's `large_futures` threshold — the same reason the inline
        // push-config branch inside it is boxed.
        let started = Box::pin(async move {
            // A halted tenant takes no new work (`halt`); a halt that lands
            // after this check is caught once the token is registered.
            self.refuse_if_halted().await?;
            // A lease and a budget are checked before any work; the lease is
            // stored once the turn has its token (`lease`, `budget`).
            let lease = self.requested_lease(&params.message)?;
            let budget = super::super::budget::requested(&params.message, call_ctx.received_at())?;
            // Under the still-held per-context lock, so it is atomic with the
            // token insert below.
            self.reject_in_flight_send(&task_id).await?;

            trace_debug!(
                task_id = %task_id,
                context_id = %context_id,
                "creating task"
            );
            let task = create::build_initial_task(
                &task_id,
                &context_id,
                stored_task.as_ref(),
                &params.message,
            );
            // The same decision `build_initial_task` makes.
            let continues = create::continues(&task_id, stored_task.as_ref());
            let known =
                basis::LeaseRead::under_claim(claim_read_at, stored_task.as_ref(), continues);
            let mut ctx = create::build_request_context(
                params.message,
                task_id.clone(),
                context_id,
                stored_task,
                params.metadata,
                self.executor_call_context(call_ctx),
            );
            // A child of the handler's shutdown token, so shutdown reaches this
            // task too — even if it began a moment ago (`cancel_in_flight`).
            ctx.cancellation_token = self.in_flight.task_token();
            ctx.approval = approval;
            ctx.budget = budget;
            ctx.content_check = content_check;
            if let Some(config) = params.configuration.as_ref() {
                ctx = ctx.with_accepted_output_modes(config.accepted_output_modes.clone());
            }
            let referenced = self.resolve_references(&ctx.message).await?;
            ctx = ctx.with_referenced_tasks(referenced);

            // From here on there is something to release on failure: the queue
            // first, then the token, then the row.
            let (writer, reader, persistence_rx, lease_read) = self
                .lease_event_queue(&task_id, use_background, !continues, known)
                .await?;
            writer.bind_identity(
                task_id.clone(),
                a2a_protocol_types::task::ContextId::new(ctx.context_id.clone()),
            );
            guard_ref.leased();
            let turn = self
                .register_cancellation_token(&task_id, ctx.cancellation_token.clone())
                .await;
            let claim = claim.map(std::sync::Arc::new);
            if let Some(claim) = &claim {
                claim.attach(&ctx.cancellation_token);
            }
            guard_ref.registered(&turn);
            // A halt that landed after the check above may have walked the
            // tokens before this one was inserted; it has set its flag by
            // now, so stop this turn the way the walk would have (`halt`).
            self.stop_if_halted(&turn, &ctx.cancellation_token);
            if let Some(lease) = lease {
                self.start_lease(&task_id.0, lease, &turn, &ctx.cancellation_token)
                    .await?;
            }
            // Before the save, so that nothing is awaited between a
            // continuation's save and its executor starting: a drop there
            // would leave the turn saved with nothing to run it, and the
            // guard rolls back only a new row (see `commit_guard`). The
            // record states a decision the gate admitted, which is as true
            // before the row exists as after.
            #[cfg(feature = "audit")]
            Box::pin(self.audit_approval(call_ctx, &task_id, ctx.approval.as_ref())).await;
            // A continuation's inline push config is registered here, before
            // its turn is saved; a new task's below (see `save_turn`).
            // Saved as the lease read it, at that read's position (`basis`).
            let (task, read_at, whole) =
                basis::based_on(task, lease_read, continues, claim.is_some());
            writer.bind_state(task.status.state, continues);
            let config = params.configuration.as_ref();
            Box::pin(self.save_turn(&task, read_at, config, continues, whole)).await?;
            if !continues {
                guard_ref.created_row();
            }

            // Boxed, and with every local confined to the helper, so this cold
            // branch does not enlarge the send future for every send — inline it
            // pushed all three dispatch futures past clippy's `large_futures`
            // threshold.
            //
            // Before the guard is dropped, not after: this step can still
            // reject the send, and it rolls the task row back when it does.
            // Dropping the guard first published a task that a concurrent
            // send for the same context could find by `find_task_by_context`
            // in the window before the rollback.
            let inline = params.configuration.as_ref().filter(|_| !continues);
            Box::pin(self.register_inline_push_config(inline, &task_id, true)).await?;

            // The context lock is released when the guard is dropped after
            // this block, in the same poll: nothing below awaits. From then
            // on a send for this context_id finds the task by
            // `find_task_by_context`. An explicit release here was removed:
            // with no await before that drop, nothing could tell them apart.

            let cancel = ctx.cancellation_token.clone();
            let gate = writer.terminal_gate();
            // On a single-worker runtime a blocking send's executor runs in
            // the task that collects its events (see `respond_blocking`):
            // two tasks on one worker cannot run in parallel, so the split
            // only costs handoffs. With more workers it gets its own task,
            // which then runs beside the collection. Measured on the echo
            // benchmark, unary sends: one worker, 14.4k -> 19.6k rps for the
            // shared task; two workers, 11.9k (own task) vs 11.4k (shared).
            let executor = if use_background || !runtime_has_one_worker() {
                ExecutorStart::Spawned(self.spawn_executor(
                    ctx,
                    writer,
                    tenant_slot,
                    turn,
                    claim.clone(),
                ))
            } else {
                ExecutorStart::Deferred(self.defer_executor(
                    ctx,
                    writer,
                    tenant_slot,
                    turn,
                    claim.clone(),
                ))
            };
            Ok(Started {
                task,
                reader,
                persistence_rx,
                executor,
                cancel,
                gate,
                claim,
                saved_whole: whole,
            })
        })
        .await;
        guard.disarm();
        drop(guard);

        match started {
            Ok(started) => Ok(Committed::Started(Box::new(started))),
            Err(err) => {
                // The send failed after taking the key. Leaving it held would
                // make the caller's legitimate retry replay to a task that
                // was never created.
                if let Some(key) = claimed_key {
                    self.release_send_key(&key).await;
                }
                Err(err)
            }
        }
    }
}
