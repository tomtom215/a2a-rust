<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0034: Task lineage is an extension, and a job is halted by its root

**Date:** 2026-10-31
**Status:** Accepted, amended 2026-11-04 (below) and built for 0.16.0. The
shape follows the maintainer's recorded decision H-D5 (a lineage extension
whose `root` defaults to the originating context). The maintainer asked on
2026-10-31 that the design be recorded for a later session, and on
2026-11-04 that it ship in 0.16.0; the amendment settles the open questions
and is the maintainer's to veto in review.
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

## What would settle it (as proposed on 2026-10-31)

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

## Amendment, 2026-11-04: the open questions settled, and what is built

The maintainer's instruction of 2026-11-04 puts every open audit item in
0.16.0, so this ADR is built rather than left Proposed (audit H10). These are
the choices the section above left open, each with the reason.

1. **Wire shape.** `Message.metadata["a2a-rust.com/lineage"]` is
   `{"root": string, "depth": integer, "parentTaskId"?: string}`, and the
   message declares `https://a2a-rust.com/extensions/lineage/v1` in
   `Message.extensions`. Unknown members are ignored, so a later `v1` may add
   optional ones. The server reads the metadata key whether or not the URI is
   declared, as it does for the lease and the budget.
2. **`parentTaskId` is carried, optionally.** It is the parent's task id on
   the parent's agent, and nothing else: no agent URL or card. A task id is an
   opaque name the child cannot use to reach the parent, so the leak the Open
   list feared (an agent location at every hop) does not happen, and with it a
   tree is built from the tasks alone, without a trace backend
   (`a2a_protocol_types::lineage::tree`). `Delegation` sets it through
   `RequestContext::child_lineage`.
3. **The root and parent are bounded strings, not unreserved characters.**
   1 to 1024 bytes, no control characters. The default root is the
   originating `contextId`, which may be any string a server accepts (the
   default `max_id_length` is 1024 bytes); a character set narrower than the
   context id's would make a child refuse the root its parent was given.
   A hop whose context ids are longer than 1024 bytes names an explicit root.
4. **The depth cap is 16** (`MAX_LINEAGE_DEPTH`), the same number as the actor
   chain of ADR 0021: a tree deeper than that already fails token exchange.
   `Lineage::child` refuses to go past it at the parent, and the server
   refuses a message past it before any work.
5. **A task's lineage is fixed when it is created.** A new task records the
   lineage its message carried in `Task.metadata` under the same key (nothing
   is written for a message that carried none, so a caller that does not use
   the extension sees no change on the wire). A continuation keeps the
   lineage recorded on its task; a continuation whose message names another
   root is refused as `InvalidParams`. A task with no recorded lineage takes
   the continuation's, or else its own `contextId`.
6. **`HaltScope::Root { tenant, root }`**, a struct variant on the
   `#[non_exhaustive]` enum: the halt names its tenant explicitly, since
   `halt` is called by operator code, not inside a tenant's request.
7. **The halt store key** of a root halt is `root:<n>:<tenant><root>`, `<n>`
   the tenant's length in bytes, in decimal. Tenants and roots may contain
   `:`, so a separator alone could not tell `("a:b", "c")` from
   `("a", "b:c")`; a length prefix can, for any two strings. The `root:`
   prefix differs from `all` and `tenant:`, so no key of one kind is a key of
   another (the second addition the triage asked for).
8. **Mixed versions.** A 0.16 replica reads a row it cannot parse as a halt
   of everything (audit NEW-h-designs-4, fixed in the same release), so an
   older 0.16 replica sees a newer scope kind as a halt of everything, which
   fails closed. A replica of 0.15 or older fails its whole load on such a
   row. The triage asked for the tolerant parsing to ship a release before
   any new scope kind; the instruction to ship both in 0.16.0 overrides that,
   so the rule moves to the operator: **do not give a root halt until every
   replica sharing the halt store runs 0.16**. The upgrade page says so.
9. **`covers` takes the running turn's tenant and root.** The registry of
   running turns keeps each turn's lineage; a halt walks it once, and the
   poller of a halt store does the same for a scope another replica wrote.
10. **The tree view** is two things. `RequestHandler::running_in_root` lists
    the turns of one root running in this process, which is what a halt of
    that root would stop here. `a2a_protocol_types::lineage::tree` arranges any set of tasks
    (`ListTasks` from each agent, a dump of the task tables) by
    `parentTaskId`, attaching a child only to a parent exactly one level up,
    so a forged or inconsistent set cannot make a cycle. A stored root index
    and a `rootId` filter on `ListTasks` stay unbuilt, for the reason in
    Decision 5.
11. **Spans** carry `a2a.lineage.root` and `a2a.lineage.depth` on the
    executor's span; no metric label carries either (audit H11).
12. **Halt refusals are machine-readable.** A send refused by any halt
    carries a `google.rpc.ErrorInfo` detail with `reason: "HALTED"` and
    `domain: "a2a-rust.com"` on every binding, beside the existing HTTP 503 /
    gRPC `UNAVAILABLE`, so a hop in another language can tell a halt from a
    server fault on JSON-RPC, where the code stays `-32603` (ADR 0049).

### Not built

* A stored root index, a `rootId` filter on `ListTasks`, and a
  protocol-level `parentTaskId` proposed upstream (outward-facing; the
  maintainer's say-so).
* Halting parked tasks: a halt by root stops none, as no halt does.
* A halt that crosses agents which do not share the halt store. Across such
  a hop a halt travels as the cancels of the parent's cascade, one hop at a
  time (ADR 0049 states what a hop in another language must do).

### What was checked (2026-11-04)

The checks the section "What would settle it" asked for, as tests:

* Two handlers on one `InMemoryHaltStore`, tasks of two roots on each:
  `handler::halt::root_tests::halting_a_root_stops_that_job_on_every_replica_and_no_other`.
  The PostgreSQL halt store runs the same store contract, root scopes
  included (`store::halt::tests::postgres_keeps_the_contract`); a two-replica
  run on PostgreSQL through the handler is not written.
* Malformed lineage and a depth past the cap refused before the executor
  runs: `a_bad_lineage_is_refused_before_the_executor_runs`, and the
  published vectors (`lineage::vectors_tests`).
* Tenant scoping: `a_root_halt_is_scoped_to_its_tenant`.
* Across a real hop: `a2a-protocol-client/tests/delegation_lineage_tests.rs`
  halts the root on the child's server and the parent's `Delegation` sees
  the child canceled.
* The key encoding: `store::halt::tests::scope_keys_round_trip_and_do_not_collide`.
* From other languages: the a2a-python and a2a-go interop clients carry a
  lineage to the echo agent and read it back (ADR 0049). The Python run found
  that a protobuf-`Struct` SDK writes the depth as `2.0`; `depth` now reads any
  whole JSON number, and the vectors include that case.
* The swarm example with a killed parent is not run.
