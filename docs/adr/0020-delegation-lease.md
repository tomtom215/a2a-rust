<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0020: A lease stops the children of a parent that went quiet

**Date:** 2026-10-18
**Status:** Accepted
**Author:** Tom F.

---

## Context

`a2a_protocol_client::delegation` cancels a child task when its parent
cancels, loses the child's stream, or drops the handle. It cannot cover a
parent that crashes, hangs, or is partitioned away: the handle dies with the
parent. `docs/swarm-orchestration.md` called this G1-B and recommended a
lease. The control map's ASI08 row (OWASP Agentic Security, "Cascading
Failures") was `partial` for this one gap: "a parent that crashes leaves its
children running".

A2A v1.0.1 has no lease, and `GetTask` carries no metadata. Any lease is
therefore an extension in the style of the idempotency, failure-class and
approval extensions.

## Options

1. **Renew by holding a stream.** A child stays alive while some
   subscription to it is open. That needs no new call, but the
   subscription can land on a different replica from the executor, and an
   SSE disconnect is detected only on the next write. Neither gives a
   bound.
2. **Renew with an explicit call, the deadline held where the executor
   runs.** Simple, but behind a load balancer a renewal landing on another
   replica never reaches the executor's. That would stop a child whose
   parent is alive, which is worse than no lease.
3. **Renew with an explicit call, the deadline held in a shared store.**
   Any replica records a renewal, and the executor's replica reads the
   deadline. It costs a store write per renewal and a store read per check.

## Decision

Option 3.

- **Grant.** A message carries `{"ttlMs": n}` in `Message.metadata` under
  `a2a-rust.com/lease`, and `https://a2a-rust.com/extensions/lease/v1` in
  `Message.extensions` (`a2a_protocol_types::lease::Lease`). A server with
  a `LeaseStore` (`RequestHandlerBuilder::with_lease_store`) declares the
  extension on its card. It refuses a grant outside 1 s
  (`MIN_LEASE_TTL`) to `with_max_lease_ttl` (default one hour) before any
  work, refuses the send if the grant cannot be stored, and stores the time
  to live and a deadline per tenant and task.
- **Renewal.** A `GetTask` of the task whose request activates the
  extension (`A2A-Extensions`) moves the deadline to now plus the time to
  live, after the read succeeds, so only a caller allowed to read the task
  can renew it. A plain read renews nothing, so a dashboard polling tasks
  cannot keep an orphan alive. `A2aClient::renew_lease` sends that request,
  keeping any extensions the client already asks for.
- **Lapse.** The replica running the turn checks the deadline every quarter
  of the time to live, at most once a second. Past the deadline it stops
  the turn as a halt does, and the task ends `canceled`. A turn that ends
  terminal releases the lease. One that ends parked keeps it for the turn
  that continues the task, whose grant replaces it.
- **Store outage.** A store that cannot be read counts as a live parent. A
  database blip must not cancel every leased child. The child then runs
  until the store answers or its own executor timeout ends it.
- **Client.** `Delegation::start_leased` attaches the grant and renews
  every third of the time to live until the handle settles, is cancelled,
  detached or dropped.
- **Stores.** `InMemoryLeaseStore` for one process, `PostgresLeaseStore`
  for replicas (`a2a_leases`, keyed by tenant and task).

## Consequences

- A child of a parent that went quiet stops within about one time to live
  plus one check. `delegation_lease_tests.rs` measures that against a real
  server, with a parent that detaches as a crashed one would, and checks
  that a live parent keeps its child running past three times the TTL.
- Over WebSocket, `A2A-Extensions` is sent only with the upgrade.
  `renew_lease` renews only on a connection that asked for the extension.
  The client docs say so.
- What stays open on ASI08: a peer that does not implement the extension
  ignores the grant, and its children run as before. Leases are opt-in on
  both sides. G5 (deadline and budget propagation) is not built. *(Built
  since: ADR 0023, 2026-10-21. Note added 2026-11-25.)*
- Tests: the store contract (in-memory and PostgreSQL), ten handler tests
  (lapse, renewal on two replicas, a plain read not renewing, bounds, a
  malformed grant, no store, an unreadable store, a refused grant,
  release, the card), three client end-to-end tests, and the types
  round-trip.

## Amendment, 2026-11-26: a parked child's lease is read by the reaper (COR-08)

The decision's *Lapse* bullet let a turn that ends parked keep its lease for
the turn that continues the task. Nothing then read that deadline: the watcher
reads only the leases of running turns, so a child parked at `input-required`
or `auth-required` whose parent died stayed parked until ADR 0056's opt-in
expiry or for ever (E03 R8).

- **Who acts.** The claim reaper, which already looks every claim time to
  live on every replica with turn claims. No second timer. On each look it
  lists the lapsed leases (`LeaseStore::lapsed_leases`, one statement on the
  SQL stores, so an idle replica's look makes one more than before) and, for a
  task no turn on that replica runs, takes the task's claim, reads the task
  and the lease again under it, and, if the task is still parked behind a
  lapsed lease, cancels it.
- **How it ends.** As `CancelTask` ends a task no turn runs, which is what ADR
  0056's *Alternatives* asked a handler-level cancel to write: the executor's
  `cancel` hook (bounded by the claim's time to live; a hook that fails or
  hangs does not keep the task parked, as a lapse stops a running turn
  without asking), the status `canceled` with the cause `lease-lapsed`, the
  ending in the log (and so the audit chain) and to the webhooks. Then the
  lease and the claim are released.
- **The key.** A lease is now kept under the task's partition (the tenant,
  and the caller when callers are isolated), the scope of its turn claim, so
  the reaper can take the claim and read the task a lease names. It was the
  tenant alone.
- **Left as they are.** A running task (its own watcher stops it), a task
  another replica holds a claim on (a turn runs it or a send continues it),
  and a live lease. A lapsed lease of a finished or missing task is left for
  the sweep's margin; the list is the most recently lapsed first, so such rows
  do not crowd out one that has just lapsed.
- **Not covered.** A handler without turn claims has no reaper, and a parked
  child there still waits for its parent, `CancelTask` or ADR 0056's expiry.
  Root halts still pass parked tasks by (register X2-7).
- **Tests.** `handler/claims/reaper/lapsed_tests.rs` (one look, in-memory:
  what it ends, leaves and releases, a hook that fails or hangs, the idle
  look's calls) and `handler/lease/parked_tests.rs` (end to end on the
  in-memory, SQLite and PostgreSQL lease stores: a quiet parent's parked
  child ends within three lease lifetimes; a renewing parent's stays).
