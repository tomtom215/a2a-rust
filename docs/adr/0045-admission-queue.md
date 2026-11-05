<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0045: An opt-in admission gate with reservations, a bounded wait, priority classes and weighted fairness

**Date:** 2026-11-04
**Status:** Accepted (0.16.0; audit H9). Built as `admission::Admission` and
`RequestHandlerBuilder::with_admission`. Off unless configured.
**Author:** Tom F.

---

## Context

Admission on a send was refuse-only, in two places (`handler/concurrency.rs`,
`handler/messaging/admission.rs`):

* a tenant's `max_concurrent_tasks` semaphore, taken with `try_acquire`;
* the event-queue manager's cap on queues (`DEFAULT_MAX_CONCURRENT_STREAMS`,
  1024), checked under the manager's map lock.

Both refuse with `Overloaded` (503 with `Retry-After`, retryable; spread by the
client since `a8bf45e`). Neither reserves anything, queues, or knows a
priority. The global cap is first come, first served: a tenant inside its own
limit can hold all 1024 slots, and every other tenant is refused until its
turns end (`tenant_config/mod.rs`, "Fairness under shared process-wide caps",
says so). A burst in one delegation tree therefore becomes refusals in every
other tree on the replica (audit H9, `docs/audits/2026-10-29/H-swarm-scale.md`
§9).

The triage (DECISIONS.md, H9) listed three options and recommended A, then B
opt-in, then C only with an authenticated source:

* **A. Reserved shares**, still refuse-only: a tenant gets a floor of the
  global capacity others cannot take.
* **B. A bounded wait**: wait for a slot for at most
  `min(max_wait, budget remaining)`, a bounded queue, FIFO per tenant and
  deficit round robin (DRR) across tenants.
* **C. Priority classes.** The audit report took the class from the message
  (`a2a-rust.com/priority`); that is the caller's own claim, so every caller
  would claim the top class.

The maintainer put H9 in 0.16.0 on 2026-11-04 (fork F12 had said "the four
fixes only").

## Decision

One gate, `Admission`, built with `RequestHandlerBuilder::with_admission`,
that does all three, configured entirely on the server:

1. **Capacity.** `Admission::new(capacity)`: how many turns may run at once
   through the gate. The permit is taken after the tenant slot and after the
   request is validated, before any side effect, and is held for the life of
   the spawned executor, as the tenant slot is. The build refuses a capacity
   of 0, and a capacity above the queue cap (`max_concurrent_streams`), since
   the queue cap would then refuse what the gate admitted.
2. **Reservations (option A).** `TenantShare::with_reserved(n)`: `n` slots
   only that tenant may use. The rest (capacity minus every reservation) is
   shared. A tenant may take a slot when it is below its reservation, or when
   the shared part has room. The build refuses reservations that sum past the
   capacity. With `max_queued` 0 the gate is refuse-only and this is option A
   on its own.
3. **A bounded wait (option B).** `with_max_queued(n)` and
   `with_max_wait(d)`. A send that cannot take a slot waits, if the queue
   holds fewer than `n` sends, for at most `d`, or for what remains of the
   send's budget (ADR 0023) if that is shorter. A full queue or a wait that
   ends at `d` is refused `Overloaded`, as before. A wait that ends because
   the budget ran out is refused as `requested` refuses a budget whose
   deadline has passed (invalid params): retrying cannot help that send.
   Defaults: `max_queued` 0, `max_wait` 0, so a gate with no queue settings
   only enforces capacity and reservations.
4. **Priority classes (option C), from server configuration only.**
   `TenantShare::with_priority(p)`: a freed slot goes to the highest class
   with an eligible waiter. Nothing the caller sends sets a class. The tenant
   is the resolved `TenantContext`, which a `TenantResolver` derives from the
   authenticated request, so the class follows what the server decided about
   the caller, not what the caller asserted.
5. **Weighted fairness.** Within a class, freed slots go round the tenants
   that are waiting, `weight` grants per tenant per round (DRR with unit cost:
   every turn costs one slot). Within a tenant, FIFO.
6. **Work conserving.** Every release and every arrival hands free slots to
   eligible waiters at once, so a waiter is only ever left waiting while it is
   ineligible; an arriving send that is eligible is admitted without queueing
   behind waiters that cannot be.
7. **Visible.** `RequestHandler::admission_load()` returns running, queued
   and capacity, which the load signal of ADR 0046 reports.

Tenant settings live on `Admission`, not on `TenantLimits`: they mean nothing
without the gate, and every `TenantLimits` field is documented as enforced
(`scripts/check_inert_bounds.py` holds the crate to that).

## Consequences

* Default behaviour is unchanged: no gate, refuse-only, as before.
* A queued send holds its tenant slot while it waits. A tenant's waiters are
  therefore bounded by its own `max_concurrent_tasks`, and a waiting send holds
  one connection and one future, nothing else.
* Strict priority can starve a lower class under sustained load in a higher
  one. That is what a class means; weights within a class are the fair part.
* Per replica, like every limit in the handler (audit E-F12). A fleet-wide
  queue is not built.
* A replay of an idempotent send and a send that ends waiting for an approval
  quorum also pass the gate before they find out; they release the slot when
  they return, as they release the tenant slot.

## Not built

* A priority taken from the message or from a header. It would need an
  authenticated source and a policy for who may claim what; the tenant
  already is one.
* Preemption: a running turn is never stopped to free a slot.
* A queue for the tenant's own `max_concurrent_tasks`: it stays refuse-only,
  the tenant's declared bound.
* Fleet-wide admission (one queue across replicas).
