<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0034: Task lineage is an extension, and a job is halted by its root

**Date:** 2026-10-31
**Status:** Proposed. Nothing here is built or run. The shape follows the
maintainer's recorded decision H-D5 (a lineage extension whose `root` defaults
to the originating context). The maintainer asked on 2026-10-31 that the
design be recorded for a later session.
**Author:** Tom F.

---

## Context

A2A has no link from a task to the tasks its agent starts elsewhere (audit
H10). A search of the code, protocol and schema files for `parent_task`,
`parentTask`, `lineage`, `root_task` and `rootId` found nothing but the
swarm document's option C. So there is no tree view and no "halt this job".

What exists:

* `RequestHandler::halt` takes a `HaltScope` of `All` or `Tenant`
  (`crates/a2a-protocol-server/src/handler/halt.rs`; the enum is
  `#[non_exhaustive]`). A halt refuses new sends of its scope and fires the
  cancellation token of the running tasks of that scope. With a halt store,
  every replica follows it (ADR 0019, ADR 0024). A task parked at
  `input-required` or `auth-required` is not running and is not cancelled.
* Cancelling a root cancels its children across Rust hops, through
  `Delegation`, while the parent process lives. A parent that crashed is
  covered by leases (ADR 0020), but only per child and only by expiry.
* Today's job-scoped stop is one of those two, or a tenant per job, which
  needs tenant-aware stores and the tenant carried by hand (and the default
  store ignores tenants, audit G3).
* `ListTasksParams` is generated from the protocol schema (ADR 0016), so a
  `rootId` filter there would diverge from it.
* The swarm document called a protocol-level `parentTaskId` "Large, and only
  meaningful upstream", to be done only if the A2A project wants it.

## Decision

An extension in the established style (lease, budget, failure, idempotency),
which also gives the protocol-level field a compatible place to map to if the
A2A project adopts one.

1. **Wire.** A message carries `{"root": "...", "depth": n}` in
   `Message.metadata` under `a2a-rust.com/lineage`, and declares
   `https://a2a-rust.com/extensions/lineage/v1` in `Message.extensions`
   (`a2a_protocol_types::lineage::Lineage`).
   * `root` names the job: a bounded string of unreserved characters. A
     message with no lineage starts one, and the server takes the
     **originating context** (the message's `contextId`) as the root, so a
     job needs no setup.
   * `depth` is the sender's depth plus one, starting at 0. It is capped; the
     cap is a number to choose (the actor chain in ADR 0021 stops at 16
     hops). A message above the cap, or with a malformed lineage, is refused as
     `InvalidParams` before any work, the way a malformed budget is. That also
     stops unbounded recursive delegation.
2. **Server.** The registry of running turns that a halt walks also holds each
   turn's lineage. `HaltScope::Root(String)` is added (additive, the enum is
   `#[non_exhaustive]`): it covers the running turns of one tenant whose root
   equals it. The scope is `(tenant, root)`, so two tenants' roots never
   collide. It goes through the halt store like the other scopes, so every
   replica applies it. New sends carrying a halted root are refused. No task
   store index is needed for this.
3. **Client.** The executor's context exposes the turn's lineage, and
   `Delegation::start*` takes the child's from it (same root, depth plus one).
   An executor that delegates passes the context in, and nothing else changes
   for it.
4. **Trust.** Lineage is a claim by the caller. It scopes a halt and labels
   spans; it is **never** used to authorize anything, the same line ADR 0021
   drew for an unsigned "on behalf of". A caller naming another job's root
   subjects only its own tasks to that job's halt, and a halt needs an
   operator's authenticated call. The root goes on spans (`a2a.lineage.root`,
   `a2a.lineage.depth`; names to check against the span reference at build
   time), and **not** on metric labels, where an id per job is unbounded
   cardinality (audit H11).
5. **Tree view** is by trace id, which exists (audit R6). A stored index by
   root, for `ListTasks`-style queries, is a store-contract and PostgreSQL
   schema change and is deferred until a measurement or an adopter needs it.

## Alternatives

| Option | Why not |
|---|---|
| A tenant per job | Isolation and job identity are different things; the default store ignores tenants; every hop would carry the tenant by hand. |
| A protocol-level `parentTaskId` | A cross-SDK change; diverges from the schema (ADR 0016) until the A2A project decides. Sending a proposal upstream is outward-facing and needs the maintainer's say-so. |
| The `contextId` alone | A caller-chosen conversation key shared across turns, and a job can span contexts. It is the *default* root, not the only one. |
| Cascade by handle only | Needs the parent alive; the gap is the parent that is not. |

## Consequences

* An agent that does not implement the extension ignores it, and the job is
  halted at that hop's edge by the existing mechanisms.
* A halt by root does not stop parked tasks, as no halt does.
* The registry of running turns grows by a lineage per turn.

## What would settle it (nothing is run)

* Two handlers on one `InMemoryHaltStore`, tasks under two roots across both:
  halting root A stops A's tasks on both and none of B's; a new send under A
  is refused and under B is accepted. The same on the PostgreSQL halt store
  with two real replicas (the repo's PostgreSQL suites).
* Malformed lineage and a depth past the cap are refused before the executor
  runs (an executor counter stays at zero).
* Tenant scoping: tenant X's root `r` is not stopped by tenant Y's halt of `r`.
* Mutation testing on the new `covers` arm and the depth check.
* The swarm example: a two-hop tree with the parent process killed, then the
  root halted by an operator, counting live executions to zero.

## Open

* Whether a task carries its parent's task id and agent, for a tree view
  without the trace. It leaks an agent location to every hop, so it is
  optional at best.
* The depth cap, and the root's length and character set.
* Whether `covers` should take the lineage or the registry should filter
  first. `covers` is private today.
* Whether a stored root index belongs in the `TaskStore` contract.
